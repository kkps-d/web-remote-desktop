# Roadmap

Open questions are marked **TBD**. What's built is described in [Architecture](ARCHITECTURE.md).

## Milestones

| Version | Scope | Status |
| --- | --- | --- |
| v0.5 | Stream the desktop to the web client, view only | Done |
| v1 | Mouse, keyboard and touch input | Done |
| v1.1 | Stream the remote computer's audio | Done |
| v1.15 | Audio capture modes (optional) | Not started |
| v2.0 | Reach agents over the internet through the orchprox | Config only |
| v3.0 | Authentication: clients log in, agents register with the orchprox using a secret | Not started |

Out of scope for now: Windows service, login screen/UAC capture, multiple monitors, clipboard, HTTPS.

## v1.15: audio capture modes (optional)

Build it only if process loopback falls short in practice, e.g. its latency or the host's speakers playing during a session.

Streaming hosts like Sunshine send the host's audio to a virtual output device, so the host stays silent and headless PCs work. We'd reuse existing devices instead of writing a driver.

Modes:

- **Process loopback** (default): as today. Nothing to install; the host's speakers keep playing; survives output device switches. About 135 ms.
- **Default-device loopback:** about 40 ms lower latency (96 ms). The agent listens for default-device changes (`IMMNotificationClient`) and restarts the audio branch, so audio drops briefly at a switch. Captures after the Windows volume, so a muted host streams silence.
- **Steam Streaming Speakers:** the virtual device installed with Steam. The host goes silent, PCs without an audio device work, and latency drops by about 40 ms. Unavailable, with the reason, when Steam's driver isn't installed.
- **Output device:** any other installed output device, e.g. VB-Cable.

The device modes:

- Make the chosen device the Windows default for the session, through the undocumented `IPolicyConfig` COM interface (as Sunshine does), and capture it with default-device loopback.
- Restore the previous default when the session ends, unless the user changed it during the session. The agent saves both devices to a file first and restores at startup after a crash, under the same condition.
- Go silent, not fail, if the user switches the default device during the session.

**TBD:** whether the mode is an agent setting or a client stream setting.

Out of scope: writing, signing or bundling an audio driver.

## v2.0: orchprox

A server on a VPS with a public IP that coordinates the handshake and relays all traffic, so agents behind CGNAT are reachable and clients never learn an agent's IP. Feasibility checked; only the `[orchprox]` setting exists.

- **Control** (signaling, settings, stats, end session): the agent keeps a long-lived outbound connection (e.g. WebSocket) to the orchprox, which forwards client requests down it. Custom code, mostly request forwarding.
- **Media and input:** relayed by a standard TURN server (e.g. coturn) on the same VPS. Both ends get it in their ICE config; the client forces relay with `iceTransportPolicy: "relay"`. Signaling stays one request.
- **Client:** the address becomes orchprox + agent ID.
- **Security:** HTTPS on the orchprox (needs a domain), short-lived TURN credentials per session.
- **Fallback:** TURN over TLS on port 443 for networks that block UDP.
- **Costs:** every stream crosses the VPS twice (about 9 GB each way per hour at 20 Mbps), so check transfer limits. Pick a VPS region close to the usual users and agents.

**TBD:** always relay, or try a direct connection first to save bandwidth. Direct often fails when both ends are behind CGNAT, so always relay is the starting point.

## v3.0: authentication

**TBD:** how clients log in and how a client finds or pairs with an agent.

## Linux

- **Unattended start (idea, on hold):** restore tokens only skip the share dialog after someone approves it once, and a lost token brings it back. Mutter's D-Bus API (`org.gnome.Mutter.RemoteDesktop` + `org.gnome.Mutter.ScreenCast`, as gnome-remote-desktop uses) has no dialog: on GNOME 50 a plain session process got a monitor stream from it. Its session also takes keyboard, pointer and touch, which could replace uinput. Pair it with a systemd user service and automatic login. Downsides: GNOME only, a private API, and the agent would share the screen with no prompt while it lacks authentication.
- **Other compositors:** KDE Plasma and wlroots compositors implement the portals differently. Touch support in the RemoteDesktop portal varies.
- **Scaling:** check coordinates and stream size with fractional scaling and several monitors.
- **GPU encoders:** VA-API and NVENC need `find_display_gpu` ported and GPU passthrough to test. Zero-copy DMABuf → VA is the Linux counterpart of the D3D11 path.
