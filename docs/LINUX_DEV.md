# Linux development

The Linux agent targets Wayland desktops with PipeWire. It captures through the xdg-desktop-portal and the audio through PipeWire. This page covers the development VM and the port, split into three steps.

The agent doesn't build on Linux yet: `src/platform/` has only a Windows module, so names like `platform::CAPTURE` don't resolve.

## The VM

- **Ubuntu 26.04 LTS desktop.** Its GNOME runs on Wayland only; there is no Xorg session. PipeWire is the audio server, and pipewire-pulse serves the PulseAudio API. It ships GStreamer 1.28.2; the 1.28.4 minimum in the README only applies to WASAPI on Windows.
- **VMware Workstation Player:**
  - At least 4 vCPUs and 8 GB of memory. Without a GPU, x264 encodes on the CPU.
  - 3D acceleration on.
  - A fixed 1920×1080 display.
  - An HD Audio sound card.
- **Desktop session:** turn on automatic login, and turn off screen blanking and locking. The agent works inside the logged-in session.
- **Network:**
  - SSH is forwarded on `localhost:2222`.
  - WebRTC media uses random UDP ports, so a viewer needs to reach the VM's own IP:
    - NAT is enough for a browser on the host.
    - Use a bridged adapter to test from LAN devices such as a tablet.
  - Ubuntu's firewall (ufw) is off by default. If you turn it on, allow TCP 8080 and UDP.

To confirm the session, run these in a terminal on the VM's desktop (not over SSH):

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

## Running from SSH

The agent must run as the desktop user, inside that user's session, to reach PipeWire and the portals:

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus WAYLAND_DISPLAY=wayland-0
cargo run -- --port 8080
```

Portal consent dialogs appear on the VM's screen; answer them in the VMware window. Run the client on Windows as usual and connect to the VM's IP.

## The port

Each step ends with a working agent, a conventional commit and a review.

### Before step 1: shared code

- Add `#[cfg(target_os = "linux")] mod linux; pub use self::linux::*;` to `src/platform/mod.rs`.
- `CAPTURE` is a constant, but on Linux the capture element needs the portal's PipeWire fd and node id at run time. Turn it into a function on both platforms.
- The pipeline test in `src/pipeline.rs` asserts the Windows strings. Make it Windows-only, or give each platform its own expected strings.
- **Decide input now, because it changes the portal session step 1 opens:**
  - **RemoteDesktop portal (recommended):** one RemoteDesktop session with ScreenCast attached gives video and input together, with one consent dialog. Input coordinates map to the stream, and no udev rules are needed.
  - **ScreenCast portal plus uinput:** works without the portal for input, but needs the `input` group and maps coordinates to the whole desktop.

### Step 1: video stream

Implement the video names in `src/platform/linux/`. Stub the rest, so the crate builds:
- `audio_unavailable()` returns `Some("not ported yet")`, which already makes sessions video-only.
- `InputInjector` and `TouchInjector` do nothing.
- `use_physical_pixels()` is a no-op.

| Name | Linux |
| --- | --- |
| Capture | Open a portal session with the `ashpd` crate: pick a monitor, set the cursor mode and a persist mode with a restore token. Then run `pipewiresrc fd=<fd> target-object=<node id> name=capture`. Keep the portal session alive for the stream, and end the stream when the user stops sharing. |
| `output_caps` | `videoconvert ! videoscale` to `video/x-raw,format=NV12,…,framerate=N/1` in system memory. |
| `show_cursor` | The portal fixes the cursor mode per session. Start with embedded and log that toggling isn't supported, or use metadata mode and draw the cursor. |
| `ENCODERS` | `x264enc` (as on Windows, without `d3d11download`), `vah264enc`/`vah264lpenc`, `nvh264enc`. Only x264 runs in the VM. |
| `find_display_gpu`, `runs_on` | `None` and CPU encoders first. VA elements register per render node (`vah264enc`, `varenderD129h264enc`, …) for later. |
| `primary_display_size` | The portal stream's size. |

Watch for these:
- **Static screens:** GNOME sends PipeWire frames only when the screen changes, while Windows captures at the requested rate. Check that a static screen still streams, using `pipewiresrc keepalive-time` or `videorate`.
- **Frame rate:** check that settings changes renegotiate the capture.

Done when the Windows client plays the VM's desktop, and changing resolution, frame rate, bitrate and encoder works.

### Step 2: input

Implement `InputBackend` and `TouchBackend` with the choice made before step 1.

| `InputMessage` | RemoteDesktop portal | uinput fallback |
| --- | --- | --- |
| `move` | `NotifyPointerMotionAbsolute` on the stream | Absolute pointer device (`ABS_X`/`ABS_Y`) |
| `moveBy` | `NotifyPointerMotion` | Relative mouse device (`REL_X`/`REL_Y`) |
| `button` | `NotifyPointerButton` | `BTN_LEFT`, `BTN_RIGHT`, … |
| `wheel` | `NotifyPointerAxis` | `REL_WHEEL_HI_RES` (also 120 per notch) |
| `key` | `NotifyKeyboardKeycode` | `KEY_*` |
| `touch` | `NotifyTouchDown`/`Motion`/`Up` on the stream | Multitouch device, protocol B, `INPUT_PROP_DIRECT` |

Both routes need a `KeyboardEvent.code` → evdev `KEY_*` table, modelled on `platform/windows/scan_codes.rs`. Linux has no injection timeout, so the Windows touch keep-alive thread isn't needed.

The uinput fallback needs access to `/dev/uinput`. The `input` group can also read every input device; fine on a dev VM, not something to ship:

```bash
echo uinput | sudo tee /etc/modules-load.d/uinput.conf
echo 'KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"' | sudo tee /etc/udev/rules.d/60-uinput.rules
sudo usermod -aG input "$USER"   # then reboot
```

Done when absolute and relative mouse, buttons, wheel, typing, `releaseAll` and touch from the tablet all work. Check with `libinput debug-events` and a desktop app.

### Step 3: audio stream

| Name | Linux |
| --- | --- |
| `audio_capture` | `pipewiresrc stream-properties="props,stream.capture.sink=true"`, which captures the monitor of the default output |
| `audio_unavailable` | `None` when PipeWire is reachable, otherwise a reason |

Try it on the VM's desktop. Play something while it runs, then play the file back:

```bash
gst-launch-1.0 -e pipewiresrc stream-properties="props,stream.capture.sink=true" num-buffers=500 ! \
  audioconvert ! opusenc ! oggmux ! filesink location=/tmp/audio.ogg
```

Watch for these:
- **The pipeline clock:** on Windows, `provide-clock=false` stopped the audio source from freezing the video when the output device switched. Check the same with `pipewiresrc`.
- **Switching output:** check that the stream follows the new default output.
- **Echo:** viewing on the VM itself echoes, as on Windows, since the monitor includes the browser's playback.

Done when a tone played in the VM reaches the client, and switching the output device doesn't stall video or audio.

## Open questions

- **Consent:** the portals ask on every new session. Restore tokens skip the dialog after the first time. Starting unattended may need Mutter's own D-Bus API, as gnome-remote-desktop uses.
- **Other compositors:** KDE Plasma and wlroots compositors implement the portals differently. Touch support in the RemoteDesktop portal varies.
- **Scaling:** check that coordinates and the stream size hold up with fractional scaling and several monitors.
- **GPU encoders:** VA-API and NVENC need GPU passthrough to test. Zero-copy DMABuf → VA is the Linux counterpart of the D3D11 path.
