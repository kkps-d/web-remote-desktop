# Architecture

How the app works today. Planned work is in the [roadmap](ROADMAP.md); measurements behind these choices are in [spikes](spikes/).

- [Overview](#overview)
- [Web client](#web-client)
- [Signaling and network](#signaling-and-network)
- [Agent](#agent)
- [Stream settings](#stream-settings)
- [Input](#input)
- [Audio](#audio)

## Overview

- `client/`: a React web client that views and controls a remote computer over WebRTC.
- `agent/`: a Rust agent on the remote computer. GStreamer does capture, encoding and WebRTC; Rust wires it together and serves signaling.
- LAN/VPN only: the client reaches the agent directly. No STUN/TURN, plain HTTP.

## Web client

- React + TypeScript + Vite, with shadcn/ui.
  - React handles the UI shell: connect screen, connection status, toolbar, settings.
  - WebRTC (`RTCPeerConnection`) lives in a plain TypeScript module outside React's render cycle.
  - The remote screen is a `<video>` element fed the incoming `MediaStream` via a ref.
  - Input goes over an `RTCDataChannel` from native DOM listeners, not React state, to keep latency low.
- Static build output, no backend of its own. The Vite dev server is used during development.

### Screens

The desktop viewer is at `/`. The game viewer at `/play/` opens the agent from
the launch URL, uses relative mouse and trackpad touch input, and sends Escape
on pointer-lock release. It uses the agent-selected encoder and caps its three
quality presets to the session limits. Both viewers share the stats overlay and
its visibility preferences.

- **Connect form:** the agent address, pre-filled with the last used one (localStorage).
- **Recent devices** below the form: up to 8 agents, newest first, each with a screenshot thumbnail and last-connected time. Click to reconnect; forget removes one. The thumbnail refreshes 2 s after connecting, every minute, and on disconnect.
- **Connection status:** connecting, connected, failed (with the reason).
- **Stream view:** scaled to fit the window, keeping aspect ratio.
- **Toolbar:** fullscreen, disconnect, stream settings, input modes, shortcuts, mute, stats.

### Stats overlay

- The stats dropdown toggles Basic and Advanced sections independently. Defaults to Basic; the choice is remembered.
  - **Basic:** stream resolution, bitrate, framerate, network RTT.
  - **Advanced:** source resolution, encode/decode time, packet loss, jitter, jitter buffer delay, dropped frames, freezes, codec, decoder, frame pacing.
- Frame pacing plots the last five seconds of intervals between video frames submitted to the browser compositor. Missed video frame callbacks leave gaps. The dashed target shows the interval for the configured stream FPS. History clears when the overlay closes, the connection changes, or the tab is hidden.
- The frame pacing canvas redraws up to 30 times per second. The latest and peak readouts update four times per second.
- Help opens a dialog explaining each statistic and its calculation.
- Values include units; unavailable metrics show `—`. Rates and timing averages use recent samples; dropped frames and freezes are session counters.

## Signaling and network

- The agent handles signaling itself, WHIP/WHEP-style. There is no separate signaling server.
  - The browser is the offerer (receive-only video and audio, plus the `input` data channel). It POSTs its SDP offer and gets the answer in the response.
  - Non-trickle ICE: candidates are gathered before sending, so one request completes signaling.
- The agent doesn't serve the web client. The client is on a different origin, so the agent handles CORS (including the preflight for `application/sdp`).
- The user enters the agent's address manually.
- See [plain HTTP limitations](KNOWN_ISSUES.md#plain-http).

### Game container registration

When the game runner sets `GAME_STREAM_AGENT_URL` and `GAME_STREAM_LAUNCH_TOKEN`, the agent opens an outbound WebSocket on the private Docker bridge. It sends the one-time token in the Authorization header and keeps the socket open until the game ends or the service disconnects. The runner sets both variables per launch. Direct desktop connections still use the HTTP API below.

### HTTP API

| Endpoint | Purpose |
| --- | --- |
| `POST /offer` | SDP offer in, SDP answer out; starts a session |
| `GET /sessions/{id}` | Source resolution, current settings, encoder list, agent limits, and why the stream has no audio (`null` if it has) |
| `PATCH /sessions/{id}` | Change stream settings while streaming |
| `DELETE /sessions/{id}` | End the session |
| `GET /sessions/{id}/stats` | Cumulative encoding elapsed time and frame count |

## Agent

- Runs in the logged-in user's session on Windows and Linux (not a Windows service). OS-specific code is under `src/platform/`.
- Signaling with axum.
- One viewer at a time: a new connection replaces the current viewer.
- Tears down the pipeline when the viewer sends `DELETE` or the WebRTC connection fails.
- `webrtcbin` matches the offer against exact RTP caps, so the agent uses the browser's payload types and matches H.264 `profile-level-id` via `capssetter`.

### Settings file

`agent.toml` next to the executable, or `--config <path>`. See [`agent.example.toml`](../agent/agent.example.toml).

- `[direct]` (port) and `[orchprox]` (url) turn each connection mode on; at least one is required. No file means direct on 8080. `--port` overrides the port and turns direct on.
- `[stream]` sets a maximum resolution (a bounding box; the stream starts scaled down to fit), default and maximum frame rate, and default and maximum bitrate.
- Set `loopback_only = true` in `[direct]` to bind signaling to `127.0.0.1`.
- Set `bind_address` in `[direct]` to bind signaling to a specific IPv4 address. `loopback_only` takes precedence.
- A viewer URL with `?agentPort=PORT` connects to the viewer's host on that port. `?localAgent=127.0.0.1:PORT` connects to loopback. Desktop launch links use the agent’s stream defaults. The manual Connect screen uses the browser’s saved stream preferences. Game launch links start at Medium quality.
- The orchprox url is only checked for now.

### Windows pipeline

`d3d11screencapturesrc ! d3d11convert ! capsfilter ! <encoder bin> ! h264parse ! rtph264pay ! capssetter ! webrtcbin`

- Captures the primary display on the GPU.
- Encoders, in default order: NVENC, AMF, Media Foundation, Direct3D 12, x264, Microsoft software. See [encoders](ENCODERS.md).
- CPU encoders get `d3d11download`; `d3d12h264enc` gets `d3d12upload`.
- Needs Windows 10 1809+ and GStreamer 1.28.4+.

### Linux pipeline

`pipewiresrc ! videorate drop-only=true ! capsfilter name=rate caps-change-mode=delayed ! videoconvert ! videoscale ! capsfilter name=output ! <encoder bin> ! …`, into NV12 in system memory.

- Targets GNOME on Wayland with PipeWire. Setup is in [Linux development](LINUX_DEV.md).
- **Portal capture:** By default, `platform::Capture` holds an `ashpd` ScreenCast portal session sharing one monitor, with the cursor embedded. Closing it removes GNOME's sharing indicator. Stopping sharing from GNOME ends the session through `on-disconnect=error`.
- **Named capture:** Linux agents with `[capture]` target a BGRx PipeWire node in the current session. The config supplies the native dimensions for stream limits. Frame rate and scaling are applied downstream. Gamescope input is opt-in via `eis_socket`. Patched gamescope builds can receive viewer cursor requests through `pipewire_cursor_socket` when EIS is enabled. Named audio is opt-in via `pipewire_audio_sink`.
- **Consent:** the first stream shows GNOME's share dialog. The offer waits up to 60 s for an answer, then fails and closes the dialog. A restore token in `~/.local/state/web-remote-desktop/screencast-restore-token` skips the dialog later, also after restarts. Delete the file to choose again.
- **Static screens:** GNOME sends frames only when the screen changes. `keepalive-time=100` resends the last one, and `videorate` caps the variable rate at the chosen frame rate.
- **Settings changes:** `pipewiresrc` answers a reconfigure event by reconnecting, after which GNOME sends no frames. `Capture::attach` drops reconfigure events before they reach it; the elements after it handle every change.
- **Encoders:** After capture opens, the agent matches NVENC and VA-API to the capture GPU and checks that each can produce baseline H.264 at its starting settings. The session defaults to NVENC, then VA-API, then x264. See [Linux encoders](ENCODERS.md#linux).

## Stream settings

- The client's toolbar picks resolution (native or smaller, same aspect ratio), frame rate (15, 30, 60 or 120 fps), bitrate and encoder. Defaults: native resolution, 30 fps, 8 Mbps.
- Options above the agent's limits are left out, and saved settings are lowered to them. Changes above the limits are rejected with a 400.
- Encoders the agent can't use are greyed out with the reason. Windows uses the primary monitor's GPU. On Linux, GNOME portal capture matches the compositor's Wayland feedback to a visible DRM device. Named PipeWire capture uses hardware when `/dev/dri` exposes one identifiable GPU.
- Each encoder's size limits come from its sink pad template (NVENC, AMF and D3D12: 4096 px at most). A change outside them is rejected with a 400 before the pipeline changes. A display larger than the default encoder accepts starts scaled down to fit.
- Encode time in stats measures encoder input-to-output latency, including internal queueing.

### Live changes

Changes apply without SDP renegotiation:

- Windows sets resolution and frame rate on the output capsfilter. Linux sets the frame rate on the filter immediately after `videorate` and resolution on the output capsfilter. The Linux rate filter temporarily accepts the previous caps so a frame already leaving `videorate` can pass. The encoder restarts with a keyframe.
- The encoder picks up a new `bitrate` on the next frame.
- Switching encoder blocks the output capsfilter's source pad for one frame and swaps the encoder bin. The new encoder starts with a keyframe.

## Input

One reliable, ordered `RTCDataChannel` named `input`, created by the client before the offer. The agent receives it via `webrtcbin`'s `on-data-channel` (connected before setting the remote description).

### Modes

Mouse, touch and rate are picked in the toolbar and remembered in the browser.

- **Mouse:**
  - **Absolute** (default): positions as 0–1 fractions of the video's content area (letterbox excluded). The client cursor is shown; the stream cursor is hidden.
  - **Relative:** movement deltas, applied 1:1 in pixels, so sensitivity doesn't change when the stream is scaled. The client cursor is hidden (Pointer Lock); the stream cursor is shown.
- **Touch:**
  - **Touchscreen** (default): every finger is sent and injected as real touch, so the OS handles pinch-zoom, inertia scrolling and press-and-hold. Fingers on the letterbox are ignored; drags off the video pin to its edge.
  - **Direct** (mouse emulation): tap clicks, drag drags with the left button, long press right-clicks, two-finger drag scrolls.
  - **Trackpad** (mouse emulation): one-finger drag moves the cursor, tap left-clicks, two-finger tap right-clicks, two-finger drag scrolls, tap then drag drags. The stream cursor is shown.
- **Rate:** moves, wheel scrolls and touch moves go out at most at 125 (default), 250, 500 or 1000 Hz.
  - The first move after a pause goes out at once; moves in the rest of that interval are merged (latest position, summed deltas, latest touch frame) and sent when it ends.
  - Buttons, keys and fingers landing or lifting go out at once, after any pending move, so they keep their order.
  - Over HTTP, browsers deliver moves once per display frame, so rates above the refresh rate change nothing.

### Keyboard and shortcuts

- The client sends `KeyboardEvent.code`; the agent injects scan codes, so the remote keyboard layout decides the characters.
- Every key goes to the remote computer, Tab included. Ctrl+Alt+Home releases held keys, leaves fullscreen and Pointer Lock, and moves focus to the toolbar.
- Keyboard Lock only works on `localhost` (in fullscreen), so a toolbar dropdown sends browser-reserved shortcuts: Win, Alt+Tab, Alt+F4, Ctrl+W, Ctrl+Shift+Esc, Ctrl+Alt+Del.
- **Stuck keys:** the client releases every held key and button when the window loses focus, the tab is hidden, or it disconnects. The agent releases them when the channel closes or the session ends.

### Protocol

JSON text messages. Numbers other than `x` and `y` are integers.

| Message | Meaning |
| --- | --- |
| `{type:"move", x, y}` | Absolute position, 0–1 of the video content area |
| `{type:"moveBy", dx, dy}` | Relative move in pixels |
| `{type:"button", button, down}` | DOM button number: 0 left, 1 middle, 2 right |
| `{type:"wheel", dx, dy}` | Scroll in wheel units (120 per notch), positive right and down like DOM deltas |
| `{type:"key", code, down}` | `KeyboardEvent.code`; unknown codes are ignored |
| `{type:"touch", contacts:[{id, x, y}], canceled?}` | Every finger on the stream (`pointerId`, 0–1 like `move`). The agent diffs it with the last frame: new ids land, missing ids lift, or are canceled with `canceled: true` |
| `{type:"releaseAll"}` | Release every held key and button, and cancel touches |
| `{type:"cursor", visible}` | Show or hide the cursor in the stream |

The stream cursor follows the pointer mode over this channel rather than `PATCH /sessions`, so it stays out of the saved stream settings.

### Windows injection

- `SendInput`. Absolute positions map to pixel centers on the primary display. Relative moves are applied to the cursor position and sent as absolute moves, so pointer acceleration never applies.
- Touch goes through a synthetic touch device (up to 10 contacts). Each contact carries an 8 px contact area, since Windows never delivers contacts without one. Held contacts are resent every 100 ms, since Windows lifts contacts that stop updating.

### Linux injection

Each desktop session creates uinput devices with the `evdev` crate, which the compositor handles like real hardware. Without access to `/dev/uinput`, the agent streams without input.

| Message | Device |
| --- | --- |
| `move` | Absolute pointer (`ABS_X`/`ABS_Y` over 0–65535), mapped onto the desktop |
| `moveBy` | Relative mouse (`REL_X`/`REL_Y`). The desktop applies its pointer acceleration; games that lock the pointer read the raw motion. |
| `button`, `wheel` | The absolute pointer. The wheel sends `REL_WHEEL_HI_RES` (120 per notch) and `REL_WHEEL` once a whole notch adds up. |
| `key` | Keyboard, through the `KeyboardEvent.code` → `KEY_*` table in `key_codes.rs`. The compositor repeats held keys itself. |
| `touch` | Touchscreen: multitouch protocol B, `INPUT_PROP_DIRECT`, 10 slots. A canceled contact lifts. |

### Gamescope input

Named capture with an `eis_socket` uses a libei connection per viewer for keyboard
and pointer events. Absolute motion maps to the native capture dimensions.
The worker releases held keys and buttons before ending emulation. Stream info
reports `inputUnavailableReason` and `touchSupported`. The client refreshes them
when the input channel closes and uses mouse emulation for touch.

## Audio

One-way: the remote computer's output goes to the client as Opus on the same `webrtcbin` connection as the video. No microphone.

- The audio branch is linked after the video, so it gets `webrtcbin`'s `sink_1` and matches the offer's audio m-line. The RTP caps use the browser's Opus payload type.
- The audio source must not provide the pipeline clock: when the device switches, that clock stalls and freezes the video.
- **Client:** adds a receive-only audio transceiver to the offer and plays the track with the video.
  - Mute toggle in the toolbar, remembered in the browser. Sessions with an agent on `localhost` start muted, since the agent would capture the playback and echo it.
  - If the browser blocks unmuted playback, it plays muted and shows the toggle as muted.
  - If the agent reports why there's no audio, the mute button is greyed out with that reason.

### Windows capture

`wasapi2src loopback=true loopback-mode=exclude-process-tree loopback-target-pid=<agent's> low-latency=true provide-clock=false continue-on-error=true ! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! opusenc bitrate=128000 frame-size=10 ! rtpopuspay pt=<offer's>`

- Process loopback captures every app except the agent. It follows output device switches; plain loopback of the default device stops for good on a switch.
- Needs Windows build 20348+ (e.g. Windows 11) and GStreamer 1.28.4+. Older builds stream video only.

### Linux capture

`pipewiresrc stream-properties="props,stream.capture.sink=true" provide-clock=false ! audio/x-raw,rate=48000,channels=2`

- Captures the monitor of the default output, and follows it when the default output switches.
- Captures before the output's volume, so a muted computer still streams audio.
- Unconstrained, `pipewiresrc` picks mono, hence the caps filter.
- `audio_unavailable()` returns a reason when GStreamer's PipeWire plugin or the PipeWire socket is missing.

Named capture takes the monitor of the configured `pipewire_audio_sink` without
falling back to the default sink. Audio has its own PipeWire connection so a
sink error stays local to that source. The audio bin also keeps flush and EOS
events local to protect the shared WebRTC transport. It must produce Opus
before the agent answers the offer. If the sink is unavailable or disconnects,
the agent keeps video and input running and reports `audioUnavailableReason`
in stream info. A new viewer session retries audio.
