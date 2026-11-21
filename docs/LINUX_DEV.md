# Linux development

The Linux agent targets GNOME on Wayland with PipeWire. It captures the screen through the xdg-desktop-portal and audio through PipeWire. How it works is in [Architecture](ARCHITECTURE.md#linux-pipeline).

Develop on a Linux VM or computer, inside its desktop session: the agent needs that session to reach PipeWire and the portals.

## The VM

- **Ubuntu 26.04 LTS desktop.** Its GNOME runs on Wayland only. PipeWire is the audio server, and pipewire-pulse serves the PulseAudio API. It ships GStreamer 1.28.2; the 1.28.4 minimum only applies to WASAPI on Windows.
- **VMware Workstation Player:**
  - At least 4 vCPUs and 8 GB of memory. Without a GPU, x264 encodes on the CPU.
  - 3D acceleration on.
  - A fixed 1920×1080 display.
  - A sound card. The default Sound Blaster (ES1371) works.
- **Desktop session:** turn on automatic login, and turn off screen blanking and locking.
- **Network:** use a bridged adapter, so the VM gets a LAN address that the host and a tablet can reach.
  - Port forwarding through NAT doesn't work: WebRTC media uses a random UDP port per session, the agent advertises its own unreachable address, and browsers hide their addresses behind `.local` names the VM can't resolve across NAT.
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

| Package | Why |
| --- | --- |
| The three `-dev` packages | The `gstreamer`, `gstreamer-sdp` and `gstreamer-webrtc` crates, found through pkg-config. No `PKG_CONFIG` variables needed. |
| plugins-bad | `webrtcbin` and the VA and NVENC encoders |
| nice | The ICE transport for `webrtcbin` |
| ugly | `x264enc` |
| pipewire | `pipewiresrc`, for screen and audio capture |
| libinput-tools | Watching injected input with `libinput debug-events` |

Check the elements:

```bash
for e in webrtcbin nicesrc dtlssrtpenc rtph264pay rtpopuspay capssetter h264parse opusenc x264enc pipewiresrc vah264enc; do
  gst-inspect-1.0 --exists $e && echo "ok      $e" || echo "missing $e"
done
```

Everything should be `ok`, except `vah264enc` without GPU passthrough.

## Input access

The agent needs write access to `/dev/uinput`. This grants it through the `input` group, which can also read every input device; fine on a dev VM, not something to ship:

```bash
echo uinput | sudo tee /etc/modules-load.d/uinput.conf
echo 'KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"' | sudo tee /etc/udev/rules.d/60-uinput.rules
sudo usermod -aG input "$USER"   # then reboot
```

`sudo libinput debug-events` shows the "Web Remote Desktop" devices and what they send.

## Running

Run the agent from a terminal in the desktop session, not over SSH:

```bash
cargo run --release --manifest-path agent/Cargo.toml
```

GNOME's share dialog appears on the desktop for the first viewer. Run the client on Windows as usual and connect to the VM's IP.
