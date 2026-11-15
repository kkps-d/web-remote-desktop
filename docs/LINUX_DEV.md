# Linux development VM

Setup for developing the Linux side of `agent-rs/`. The agent doesn't build on Linux yet: `src/platform/` has only a Windows module, so names like `platform::CAPTURE` don't resolve. Adding `src/platform/linux/` is the first task.

## The VM

- **Ubuntu 26.04 LTS desktop, GNOME on Wayland.** It's the current LTS and ships GStreamer 1.28.2. Its GNOME has no X11 session, and Wayland is what Linux users run. The 1.28.4 minimum in the README only applies to WASAPI on Windows.
- **VMware Workstation Player** (the VM in `AGENTS.md`):
  - At least 4 vCPUs and 8 GB of memory. Without a GPU, x264 encodes on the CPU.
  - 3D acceleration on.
  - A fixed 1920×1080 display.
  - An HD Audio sound card.
- **Desktop session:** turn on automatic login, and turn off screen blanking and locking. The agent captures and injects into the logged-in session.
- **Network:**
  - SSH is forwarded on `localhost:2222`.
  - WebRTC media uses random UDP ports, so a viewer needs to reach the VM's own IP:
    - NAT is enough for a browser on the host, which reaches the guest IP directly.
    - Use a bridged adapter to test from LAN devices such as a tablet.
  - Ubuntu's firewall (ufw) is off by default. If you turn it on, allow TCP 8080 and UDP.

## Packages

```bash
sudo apt update
sudo apt install build-essential pkg-config git curl \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libgstreamer-plugins-bad1.0-dev \
  gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-nice \
  gstreamer1.0-pipewire gstreamer1.0-pulseaudio pulseaudio-utils \
  open-vm-tools-desktop evtest libinput-tools
```

What each group is for:
- **The three `-dev` packages:** the `gstreamer`, `gstreamer-sdp` and `gstreamer-webrtc` crates, found through pkg-config. No `PKG_CONFIG` variables are needed on Linux.
- **plugins-bad:** `webrtcbin`, DTLS/SRTP and the VA and NVENC encoders.
- **nice:** the ICE transport for `webrtcbin`.
- **ugly:** `x264enc`.
- **pipewire and pulseaudio:** screen and audio capture.
- **evtest and libinput-tools:** checking injected input.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Input access

The candidate for injecting input is uinput, which works under any compositor. Let your user open `/dev/uinput`:

```bash
echo uinput | sudo tee /etc/modules-load.d/uinput.conf
echo 'KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"' | sudo tee /etc/udev/rules.d/60-uinput.rules
sudo usermod -aG input "$USER"
```

Reboot. The `input` group can also read every input device, which is fine on a dev VM but not something to ship as is.

## Running from SSH

The agent must run as the desktop user, inside that user's session, to reach PipeWire, PulseAudio and the portals:

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus WAYLAND_DISPLAY=wayland-0
cargo run -- --port 8080
```

Portal consent dialogs appear on the VM's screen; answer them in the VMware window. Run the client on Windows as usual and connect to the VM's IP.

## What replaces each Windows part

| Windows (`platform/windows/`) | Linux candidate |
| --- | --- |
| `CAPTURE`: `d3d11screencapturesrc` | `pipewiresrc` on a node from the xdg-desktop-portal ScreenCast (or RemoteDesktop) session |
| `output_caps`: D3D11 NV12 | `video/x-raw,format=NV12` in system memory for x264. DMABuf or VA memory for VA-API |
| `show_cursor`: the `show-cursor` property | The portal's cursor mode (embedded, metadata or hidden), set when the session starts |
| `audio_capture`: `wasapi2src` process loopback | `pulsesrc device=<default sink>.monitor` through pipewire-pulse, or `pipewiresrc` capturing the sink |
| `audio_unavailable` | None when a monitor source exists |
| `ENCODERS` | `x264enc` (the only one in a VM without a GPU), `vah264enc`/`vah264lpenc` (Intel/AMD), `nvh264enc` (NVIDIA), `openh264enc` |
| `find_display_gpu`, `runs_on`: adapter LUID | VA elements register per render node (`vah264enc`, `varenderD129h264enc`, …) |
| `primary_display_size` | The ScreenCast stream's size |
| `use_physical_pixels` | Probably nothing. Stream sizes are in pixels; check with fractional scaling |
| `InputInjector`: `SendInput`, set 1 scan codes | uinput keyboard and absolute pointer, with a `KeyboardEvent.code` → evdev `KEY_*` table. Or the RemoteDesktop portal |
| `TouchInjector`: synthetic pointer device | uinput multitouch device, protocol B (`ABS_MT_SLOT`, `ABS_MT_TRACKING_ID`, `INPUT_PROP_DIRECT`) |

Candidate crates: `ashpd` for the portals, and `evdev` (uinput support) for injection.

## Check the VM is ready

```bash
gst-inspect-1.0 --version
for e in webrtcbin nicesrc dtlssrtpenc rtph264pay rtpopuspay capssetter h264parse opusenc x264enc pipewiresrc pulsesrc vah264enc; do
  gst-inspect-1.0 --exists $e && echo "ok      $e" || echo "missing $e"
done
```

Everything should be `ok`. The exception is `vah264enc`, which is missing without GPU passthrough.

```bash
gst-launch-1.0 -e videotestsrc num-buffers=300 ! video/x-raw,width=1920,height=1080,framerate=30/1 ! \
  x264enc tune=zerolatency speed-preset=ultrafast key-int-max=60 bitrate=8000 ! h264parse ! mp4mux ! filesink location=/tmp/video.mp4
```

```bash
gst-launch-1.0 -e pulsesrc device="$(pactl get-default-sink).monitor" num-buffers=500 ! \
  audioconvert ! opusenc ! oggmux ! filesink location=/tmp/audio.ogg
```

Play something in the VM while the audio test runs, then play back both files. Screen capture has no `gst-launch-1.0` test, since only a portal session can provide the PipeWire node.

```bash
ls -l /dev/uinput   # group input, crw-rw----
id                  # includes input
```

```bash
cd agent-rs && cargo build
```

This fails until a Linux platform module exists. `cargo fmt --check` works already.

## Open questions

- **Consent:** the portals ask on every new session. Restore tokens skip the dialog after the first time. Starting unattended may need Mutter's own D-Bus API, as gnome-remote-desktop uses.
- **Input:** uinput needs the `input` group but works everywhere. The RemoteDesktop portal needs no permissions, but shows a consent dialog for each session.
- **Cursor:** the portal fixes the cursor mode per session, but `cursor` messages toggle it live. Metadata mode would let the agent draw the cursor or leave it out.
- **Coordinates:** an absolute uinput pointer spans the whole desktop. Check how it maps to the captured monitor with several monitors and fractional scaling.
- **Encoders:** VA-API and NVENC need GPU passthrough to test. Zero-copy DMABuf → VA is the Linux counterpart of the D3D11 path.
- **X11:** decide whether it matters. It would use `ximagesrc` and XTest instead.
