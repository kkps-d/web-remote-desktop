# Linux development

The Linux agent targets Wayland desktops with PipeWire. It captures through the xdg-desktop-portal and the audio through PipeWire. This page covers the development VM and the port, split into three steps.

Develop on a Linux VM or computer, inside its desktop session: the agent needs that session to reach PipeWire and the portals.

## The VM

- **Ubuntu 26.04 LTS desktop.** Its GNOME runs on Wayland only; there is no Xorg session. PipeWire is the audio server, and pipewire-pulse serves the PulseAudio API. It ships GStreamer 1.28.2; the 1.28.4 minimum in the README only applies to WASAPI on Windows.
- **VMware Workstation Player:**
  - At least 4 vCPUs and 8 GB of memory. Without a GPU, x264 encodes on the CPU.
  - 3D acceleration on.
  - A fixed 1920×1080 display.
  - A sound card. The default Sound Blaster (ES1371) works.
- **Desktop session:** turn on automatic login, and turn off screen blanking and locking. The agent works inside the logged-in session.
- **Network:**
  - Use a bridged adapter, so the VM gets a LAN address that the host and a tablet can reach.
  - Port forwarding through NAT doesn't work:
    - WebRTC media uses a random UDP port per session.
    - The agent advertises its own address, which viewers can't reach.
    - Browsers hide their addresses behind `.local` names, which the VM can't resolve across NAT.
  - Ubuntu's firewall (ufw) is off by default. If you turn it on, allow TCP 8080 and UDP.

To confirm the session, run these in a terminal on the VM's desktop:

```bash
echo $XDG_SESSION_TYPE                       # wayland
wpctl status | head -n 3                     # PipeWire
systemctl --user is-active xdg-desktop-portal xdg-desktop-portal-gnome
```

## Packages

```bash
sudo apt update
sudo apt install build-essential pkg-config git curl \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libgstreamer-plugins-bad1.0-dev \
  gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-nice \
  gstreamer1.0-pipewire open-vm-tools-desktop libinput-tools
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

What each group is for:
- **The three `-dev` packages:** the `gstreamer`, `gstreamer-sdp` and `gstreamer-webrtc` crates, found through pkg-config. No `PKG_CONFIG` variables are needed on Linux.
- **plugins-bad:** `webrtcbin` and the VA and NVENC encoders.
- **nice:** the ICE transport for `webrtcbin`.
- **ugly:** `x264enc`.
- **pipewire:** `pipewiresrc`, for both screen and audio capture.
- **libinput-tools:** watching injected input with `libinput debug-events`.

Check the elements:

```bash
for e in webrtcbin nicesrc dtlssrtpenc rtph264pay rtpopuspay capssetter h264parse opusenc x264enc pipewiresrc vah264enc; do
  gst-inspect-1.0 --exists $e && echo "ok      $e" || echo "missing $e"
done
```

Everything should be `ok`. The exception is `vah264enc`, which is missing without GPU passthrough.

## Running

Run the agent from a terminal in the desktop session:

```bash
cargo run -- --port 8080
```

Portal consent dialogs appear on the desktop. Run the client on Windows as usual and connect to the VM's IP.

## The port

Each step ends with a working agent, a conventional commit and a review.

### Step 1: video stream (done)

`src/platform/linux/` streams the desktop with x264.

- **Input route:** the ScreenCast portal for video, and uinput for input in step 2.
- **Capture:** `platform::Capture` holds an `ashpd` ScreenCast session sharing one monitor, with the cursor embedded, for as long as the stream runs. Closing it removes GNOME's sharing indicator. Stopping sharing from GNOME ends the viewer's session, through `on-disconnect=error`. The pipeline runs `pipewiresrc fd=<fd> path=<node id> keepalive-time=100 on-disconnect=error ! videorate drop-only=true ! videoconvert ! videoscale` into NV12 in system memory.
- **Consent:** the first stream shows GNOME's share dialog. The offer waits up to 60 seconds for an answer, then fails and closes the dialog. The restore token in `~/.local/state/web-remote-desktop/screencast-restore-token` skips the dialog later, also after restarts. Delete the file to choose again.
- **Static screens:** GNOME sends frames only when the screen changes. `keepalive-time` resends the last one, and `videorate` caps the variable rate at the chosen frame rate.
- **Settings changes:** `pipewiresrc` answers a reconfigure event by reconnecting its stream, after which GNOME sends no frames. `Capture::attach` drops reconfigure events before they reach it; the elements after it handle every change.
- **Cursor:** the portal fixes the cursor mode per session, so `show_cursor` can't hide it.
- **Encoders:** x264 only. VA and NVENC are listed but unavailable until `find_display_gpu` is ported; VA elements register per render node (`vah264enc`, `varenderD129h264enc`, …).

### Step 2: input (done)

Each session creates uinput devices with the `evdev` crate, and the compositor handles them like real hardware:

| `InputMessage` | Device |
| --- | --- |
| `move` | Absolute pointer (`ABS_X`/`ABS_Y` over 0–65535), which the compositor maps onto the desktop |
| `moveBy` | Relative mouse (`REL_X`/`REL_Y`). The desktop applies its pointer acceleration; games that lock the pointer read the raw motion. |
| `button`, `wheel` | The absolute pointer. The wheel sends `REL_WHEEL_HI_RES` (120 per notch) and `REL_WHEEL` once a whole notch adds up. |
| `key` | Keyboard, through the `KeyboardEvent.code` → `KEY_*` table in `key_codes.rs`. The compositor repeats held keys itself. |
| `touch` | Touchscreen: multitouch protocol B, `INPUT_PROP_DIRECT`, 10 slots. Linux has no touch cancel, so a canceled contact lifts. |

Without access to `/dev/uinput`, the agent logs a warning and streams without input. Access comes from the `input` group, which can also read every input device; fine on a dev VM, not something to ship:

```bash
echo uinput | sudo tee /etc/modules-load.d/uinput.conf
echo 'KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"' | sudo tee /etc/udev/rules.d/60-uinput.rules
sudo usermod -aG input "$USER"   # then reboot
```

`sudo libinput debug-events` shows the "Web Remote Desktop" devices and what they send. Absolute and touch coordinates cover the whole desktop, which matches the stream with one monitor only.

### Step 3: audio stream (done)

`audio_capture()` runs `pipewiresrc stream-properties="props,stream.capture.sink=true" provide-clock=false ! audio/x-raw,rate=48000,channels=2`, which captures the monitor of the default output. `audio_unavailable()` returns a reason when GStreamer's PipeWire plugin or the PipeWire socket is missing.

- **Follows the default output:** switching the default output moves the capture to the new one, with no stall in audio or video.
- **Before the volume:** the monitor captures before the output's volume, so a muted VM still streams audio.
- **Stereo:** unconstrained, `pipewiresrc` picks mono, hence the caps filter.
- **The pipeline clock:** without `provide-clock=false`, the audio source would be the pipeline clock, as on Windows.
- **Echo:** viewing on the VM itself echoes, since the monitor includes the browser's playback. Linux has no equivalent of Windows' process-loopback exclusion.

## Open questions

- **Consent:** the portals ask on every new session. Restore tokens skip the dialog after the first time. Starting unattended may need Mutter's own D-Bus API, as gnome-remote-desktop uses.
- **Other compositors:** KDE Plasma and wlroots compositors implement the portals differently. Touch support in the RemoteDesktop portal varies.
- **Scaling:** check that coordinates and the stream size hold up with fractional scaling and several monitors.
- **GPU encoders:** VA-API and NVENC need GPU passthrough to test. Zero-copy DMABuf → VA is the Linux counterpart of the D3D11 path.
