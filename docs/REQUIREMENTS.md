# Requirements

Running summary of requirement discussions. Open questions are marked **TBD**.

## Web client

- Browser-based WebRTC client that connects to the agent running on the remote computer.
- Built with React (+ TypeScript, Vite) and shadcn/ui, per the project guidelines.
  - React handles the UI shell: connect screen, connection status, toolbar, settings.
  - WebRTC (`RTCPeerConnection`) lives in a plain TypeScript module or a single hook, outside React's render cycle.
  - Remote screen is a `<video>` element fed the incoming `MediaStream` via a ref.
  - Mouse/keyboard input goes over an `RTCDataChannel` from native DOM listeners, not React state, to keep latency low.
- Statically hostable build output; no backend of its own. The Vite dev server is used during development.
- **TBD:** authentication and how a client finds or pairs with an agent.

## Signaling and network

- LAN/VPN only for now: the client must be able to reach the agent directly. No STUN/TURN needed.
- The agent handles signaling itself; there is no separate signaling server.
  - The client POSTs its SDP offer to the agent (WHIP/WHEP-style) and receives the SDP answer in the response.
  - Non-trickle ICE: candidates are gathered before sending, so one request/response completes signaling.
  - The agent exposes only this signaling endpoint; it does not serve the web client.
  - The client is served from a different origin, so the agent must handle CORS (including the preflight for `application/sdp`).
- The user enters the agent's address manually in the client.
- Plain HTTP first; upgrade to HTTPS later.
  - See [plain HTTP limitations](KNOWN_ISSUES.md#plain-http).

## Agent

- Windows only for now; keep capture and input code isolated so other OSes can be added later.
- C# (.NET 10) console app running in the logged-in user's session (not a Windows service).
- GStreamer does capture, encoding and WebRTC; C# only wires it together.
  - Bindings: `gstreamer-sharp-netcore` 0.0.8 (the only `gstreamer-sharp` package on NuGet).
  - Pipeline: `d3d11screencapturesrc ! d3d11convert ! <encoder bin> ! h264parse ! rtph264pay ! capssetter ! webrtcbin`.
  - Encoder: one of the H.264 picks in [ENCODERS.md](ENCODERS.md), chosen per session; the default is the first available (NVENC, AMF, Media Foundation, Direct3D 12, x264, Microsoft software).
- Signaling: ASP.NET Core minimal API, `POST /offer` with CORS.
  - The browser is the offerer (receive-only video + `input` data channel).
  - The agent passes the offer to `webrtcbin`, waits for ICE gathering to complete, and returns the answer.
- Input: data channel messages (via `webrtcbin`'s `on-data-channel`) are translated into `SendInput` calls via P/Invoke.
- Fallback if `webrtcbin` from C# becomes too painful: keep GStreamer for capture/encoding, use SIPSorcery (pure C# WebRTC) for WebRTC.

### Spike results (`spikes/GstSpike`)

The C# + GStreamer + `webrtcbin` approach works: the desktop (2560×1440) streams to Chrome via one HTTP POST, encoded on the GPU by `mfh264enc`. Requires GStreamer 1.28.4 or later (official MSVC build, "Complete" install); earlier 1.28 releases hang with process loopback audio. See the [GStreamer workarounds](KNOWN_ISSUES.md#gstreamer-workarounds).

Out of scope for now: Windows service, login screen/UAC capture, multiple monitors. Audio comes in v1.1.

## Milestones

- **v0.5:** capture the desktop on the agent and stream it to the web client. View only, no input.
- **v1:** v0.5 plus mouse and keyboard input over the data channel.
- **v1.1:** stream the remote computer's audio to the web client.
- **v1.15 (optional):** audio capture modes: process loopback, default-device loopback, Steam's virtual speakers, or another output device.
- **v2.0:** reach agents over the internet through the orchprox.
- **v3.0:** authentication: clients log in, and agents register with the orchprox using a secret.

### v0.5 scope

- Repo layout: `agent/` (C#), `client/` (React); `spikes/` kept for reference.
- Client:
  - Connect form for the agent address, pre-filled with the last used address (localStorage).
  - Recent devices below the form: up to 8 agents the client connected to, newest first, each with a screenshot thumbnail and last-connected time. Click to reconnect; forget removes one. Stored in localStorage; the thumbnail refreshes 2 s after connecting, every minute, and on disconnect.
  - Connection status: connecting, connected, failed (with the reason).
  - Stream view scaled to fit the window, keeping aspect ratio.
  - Fullscreen and Disconnect buttons.
  - Stats dropdown independently toggles Basic and Advanced overlay sections, separated by a subtle divider. Defaults to Basic only; the choice is remembered in the browser.
    - Help opens a dialog explaining each statistic and its calculation.
    - Basic: stream resolution, bitrate, framerate, network RTT.
    - Advanced: source resolution, encode/decode time, packet loss, jitter, jitter buffer delay, dropped frames, freezes, codec, decoder.
    - Values include units; unavailable metrics show `—`. Rates and timing averages use recent samples; dropped frames and freezes are session counters.
- Agent:
  - Listens on all interfaces on a default port (8080), overridable via command-line argument or `appsettings.json`.
  - One viewer at a time: a new connection replaces the current viewer.
  - Tears down the pipeline when the viewer disconnects (client `DELETE` on the session, WHEP-style) or the WebRTC connection fails.
- Out of scope for v0.5: input, auth, HTTPS.

### Stream settings

- Client: a toolbar picker for resolution (native or smaller, same aspect ratio), frame rate (15, 30, 60 or 120 fps), bitrate and encoder; the stats overlay shows source and stream resolution.
- Encoders the agent can't use are greyed out with the reason. GPU encoders count only on the GPU driving the captured display (found with the GStreamer device monitor); others would copy every frame between GPUs. See [known encoder issues](KNOWN_ISSUES.md#encoders).
- Each encoder's size limits come from its sink pad template (NVENC, AMF and D3D12: 4096 px at most). A resolution or encoder change outside them is rejected with a 400 before the pipeline changes. A display larger than the default encoder accepts starts scaled down to fit.
- Agent: `GET /sessions/{id}` returns the source resolution, current settings and the encoder list; `PATCH /sessions/{id}` changes them while streaming. Defaults: native resolution, 30 fps, 8 Mbps.
- Agent: `GET /sessions/{id}/stats` returns cumulative encoding elapsed time and frame count. Encode time measures encoder input-to-output latency, including internal queueing.
- Changes apply live, without SDP renegotiation: new caps on the capsfilter after `d3d11convert` rescale and change the capture rate (the encoder restarts with a keyframe), and the encoder picks up a new `bitrate` on the next frame. Switching encoder blocks the capsfilter's output for one frame and swaps the encoder bin; the new encoder starts with a keyframe.

### v1 scope: input

- Transport: one reliable, ordered `RTCDataChannel` named `input`, created by the client before the offer. The agent receives it via `webrtcbin`'s `on-data-channel` and injects input with `SendInput`.
- Mouse modes, picked in the toolbar and remembered in the browser:
  - **Absolute** (default): the client sends positions as 0–1 fractions of the video's content area (letterbox excluded). The agent maps them to the captured monitor. The client cursor is shown; the stream cursor is hidden.
  - **Relative**: the client sends movement deltas, applied to the remote cursor 1:1 in pixels, so sensitivity doesn't change when the stream is scaled down or letterboxed. The client cursor is hidden (Pointer Lock); the stream cursor is shown.
- Touch modes, picked in the toolbar and remembered in the browser, independent of the mouse mode:
  - **Touchscreen** (default): the client sends every finger on the stream; the agent injects real Windows touch through a synthetic touch device (up to 10 contacts), so Windows handles pinch-zoom, inertia scrolling and press-and-hold. Fingers landing on the letterbox are ignored; drags off the video pin to its edge. Each contact carries an 8 px contact area; Windows accepts contacts without one but never delivers them. Held contacts are resent every 100 ms, since Windows lifts contacts that stop updating.
  - **Direct** (mouse emulation): tap clicks at the touch point, drag drags with the left button, long press right-clicks, two-finger drag scrolls.
  - **Trackpad** (mouse emulation): one-finger drag moves the cursor, tap left-clicks, two-finger tap right-clicks, two-finger drag scrolls, tap then drag drags with the left button. The stream cursor is shown, as with a relative mouse.
- Mouse: left, right and middle buttons, and the wheel.
- Rate: the client sends pointer moves, wheel scrolls and touch moves at most at the picked rate: 125 (default), 250, 500 or 1000 Hz, picked in the toolbar and remembered in the browser. Over HTTP, browsers deliver moves only once per display frame, so rates above the refresh rate change nothing. The first move after a pause goes out at once; moves within the rest of that interval (8 ms at 125 Hz) are merged and sent when it ends (latest position, summed relative deltas, summed wheel deltas, latest touch frame). Buttons, keys and fingers landing or lifting go out at once, after any pending move, so they keep their order.
- Keyboard: the client sends `KeyboardEvent.code`; the agent injects scan codes, so the remote keyboard layout decides the characters.
- Leaving the viewer: every key goes to the remote computer, Tab included, so Ctrl+Alt+Home (as in Remote Desktop) releases held keys, leaves fullscreen and Pointer Lock, and moves focus to the toolbar.
- Browser-reserved shortcuts: the client stays on HTTP, so Keyboard Lock only works when it runs on `localhost` (in fullscreen). A toolbar dropdown sends them as buttons: Win, Alt+Tab, Alt+F4, Ctrl+W, Ctrl+Shift+Esc, Ctrl+Alt+Del.
- Stuck keys: the client releases every held key and button when the window loses focus, the tab is hidden, or it disconnects. The agent releases them when the data channel closes or the session ends.
- Out of scope for v1: auth (v3.0), HTTPS, clipboard, audio (v1.1). See [input limitations](KNOWN_ISSUES.md#input).

#### Input protocol

JSON text messages on the `input` channel. Numbers other than `x` and `y` are integers.

| Message | Meaning |
| --- | --- |
| `{type:"move", x, y}` | Absolute position, 0–1 of the video content area |
| `{type:"moveBy", dx, dy}` | Relative move in pixels |
| `{type:"button", button, down}` | DOM button number: 0 left, 1 middle, 2 right |
| `{type:"wheel", dx, dy}` | Scroll in Windows wheel units (120 per notch), positive right and down like DOM deltas |
| `{type:"key", code, down}` | `KeyboardEvent.code`; unknown codes are ignored |
| `{type:"touch", contacts:[{id, x, y}], canceled?}` | Every finger on the stream (`pointerId`, 0–1 like `move`). The agent diffs it with the last frame: new ids land, missing ids lift, or are canceled with `canceled: true` |
| `{type:"releaseAll"}` | Release every held key and button, and cancel touches |
| `{type:"cursor", visible}` | Show or hide the cursor in the stream |

- The agent maps absolute positions to pixel centers on the primary display. It applies relative moves to the cursor position and moves there absolutely, so Windows pointer acceleration never applies.
- The stream cursor follows the pointer mode over the channel rather than `PATCH /sessions`, so it stays out of the saved stream settings.

#### Data channel spike results (branch `spike/data-channel`)

`webrtcbin` data channels work from C#. Tested on localhost with a 2560×1440 display at 100% scaling:

- The bindings have no `GstWebRTCDataChannel` wrapper, so the agent connects `on-data-channel`, `on-open`, `on-close` and `on-message-string` with `g_signal_connect_data` and replies with `gst_webrtc_data_channel_send_string` (P/Invoke). Connect `on-data-channel` before setting the remote description.
- Round trip (ping echo): 0.6 ms median. A burst of 2,000 JSON messages, each injected with `SendInput`, was handled in about 100 ms.
- Absolute and relative moves landed on the exact pixel. Relative moves skip acceleration only because "Enhance pointer precision" was off; to keep 1:1 movement with it on, the agent should apply deltas to the current cursor position and move absolutely.
- `show-cursor` on `d3d11screencapturesrc` can be toggled while streaming.
- Closing the peer connection makes `sctpenc` post a pipeline error ("SCTP association went into error state"), which ends the session as a failure. v1 should treat it as the channel closing.
- Rate caps, fed 2,000 synthetic moves/s for 5 s (agent CPU is % of one core; capture and encode alone: 2.2%):

  | Cap | Sent/s (abs / rel) | Agent CPU (abs / rel) |
  | --- | --- | --- |
  | 125 Hz | 120 / 120 | 6.0% / 9.3% |
  | 250 Hz | 230 / 234 | 6.9% / 8.8% |
  | 500 Hz | 427 / 429 | 14.0% / 14.5% |
  | 1000 Hz | 748 / 762 | 17.0% / 11.8% |
  | None | 2,000 / 1,392 | 41.4% / 14.8% |

  - The agent takes 120–180 µs per real move, mostly `SendInput`.
  - Ping round trips stayed at 0.3 ms median, 0.7 ms p95 or less, at every rate.
  - Merging lost nothing: relative circles ended on the same pixel at every cap.
  - Browser timers can't wait less than about 1 ms, so the 500 and 1000 Hz caps fall short.
- Over plain HTTP from a LAN address, Chrome lacks `pointerrawupdate` and `getCoalescedEvents` (secure context only). `pointermove` then arrives once per display frame, so a real mouse sends at most the display's refresh rate.
- Absolute moves can land 1 px off (1480 → 1479); map fractions to pixel centers.
- Not tested: button clicks (to avoid clicking on the live desktop), keyboard, touch, Pointer Lock, display scaling other than 100%.

### v1.1 scope: audio

- Agent: capture what every app plays (WASAPI process loopback, excluding the agent's own process), encode it as Opus and send it on the same `webrtcbin` connection as the video.
  - Process loopback needs Windows build 20348 or later (e.g. Windows 11) and GStreamer 1.28.4 or later (earlier 1.28 releases hang with `loopback-target-pid`). Older Windows builds stream video only.
  - Process loopback follows output device switches; plain loopback of the default device stops for good on a switch.
  - The audio source must not provide the pipeline clock: when the device switches, that clock stalls and freezes the video.
  - Like the video, the RTP caps must match the browser's offer: use its Opus payload type.
- Client: adds a receive-only audio transceiver to the offer and plays the track with the video.
  - Mute toggle in the toolbar, remembered in the browser. Sessions with an agent on `localhost` start muted, since the agent would capture the playback and echo it.
  - If the browser blocks unmuted playback, play muted and show the toggle as muted.
- Audio is one-way: no microphone from the client.

#### Audio spike results (branch `spike/audio`)

Pipeline branch, linked after the video so it gets `webrtcbin`'s `sink_1` and matches the offer's audio m-line:
`wasapi2src loopback=true loopback-mode=exclude-process-tree loopback-target-pid=<agent's> low-latency=true provide-clock=false continue-on-error=true ! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! opusenc bitrate=128000 frame-size=10 ! rtpopuspay pt=<offer's> ! webrtc.`
Tested on localhost with the client muted, so the loopback didn't capture its own playback, and from a second device on the LAN:

- Works first try: the answer has `OPUS/48000/2` as `sendonly`, and both tracks share one `msid` stream, so Chrome lip-syncs them. Lip sync looked right from the second device.
- Silence: both loopback modes keep producing samples when nothing plays, so the caps wait in `Answer()` needs no change. A steady 100 packets/s, 129 kbps, no concealment.
- Output device switch:
  - Default-device loopback (no `loopback-mode`): the stream froze, video included, with no pipeline error. With `provide-clock=false` the video kept going, but audio stopped at the switch and never came back.
  - Process loopback: audio packets never dropped across switching, muting and switching back (15 min). A beep afterwards came through at full level.
- Level: process loopback captures before the Windows volume and mute (a 0.15 beep arrived at 0.155). Default-device loopback captured after the volume (0.03).
- Latency, from a beep played on the agent machine to its arrival in the browser: 96 ms with default-device loopback, 135–138 ms with process loopback. It peaked at 172 ms right after a device switch, then settled within 30 s. Includes the agent's output path (40 ms reported by Chrome) and Chrome's audio jitter buffer (30–38 ms). The remote listener also hears their own output latency.
- Video latency: unchanged. The video jitter buffer was 5–10 ms with audio and 13 ms without, so lip sync doesn't hold the video back.
- CPU: the audio branch alone uses 0.6% of one core. With the full agent the difference is lost in noise (0.6–7% with audio, 0.8–4.2% without).
- Autoplay: clicking Connect counts as user activation, so unmuted playback starts, also after a reload on the second device. Without activation Chrome blocks unmuted `play()`, which would stop the video too.
- Not tested: whether the sound itself (not just packets) continued while on the other device and while muted, because the client was muted to avoid echo.

### v1.15 scope: audio capture modes (optional)

Optional: build it only if v1.1's process loopback falls short in practice, e.g. its latency or the host's speakers playing during a session.

Streaming hosts like Sunshine send the host's audio to a virtual output device, so the host stays silent and headless PCs work. We reuse existing devices instead of writing a driver.

- Modes:
  - **Process loopback** (default): as in v1.1. Nothing to install; the host's speakers keep playing; survives output device switches. About 135 ms in the spike.
  - **Default-device loopback**: about 40 ms lower latency (96 ms in the spike). The agent listens for default-device changes (`IMMNotificationClient`, documented) and restarts the audio branch on the new device, so audio drops briefly at a switch. Captures after the Windows volume, so a muted host streams silence.
  - **Steam Streaming Speakers**: the virtual device installed with Steam. The host goes silent, PCs without an audio device work, and latency drops by about 40 ms (default-device loopback instead of process loopback). Unavailable, with the reason, when Steam's driver isn't installed.
  - **Output device**: any other installed output device, e.g. VB-Cable.
- The device modes make the chosen device the Windows default for the session, through the undocumented `IPolicyConfig` COM interface (as Sunshine does), and capture it with default-device loopback.
  - The previous default is restored when the session ends, unless the user changed the default during the session. The agent saves both devices to a file first and restores at startup if it crashed mid-session, under the same condition.
  - If the user switches the default device during the session, the stream goes silent; it doesn't fail.
- **TBD:** whether the mode is an agent setting or a client stream setting.
- Out of scope: writing, signing or bundling an audio driver.

### v2.0 scope: orchprox

A server on a VPS with a public IP that coordinates the handshake and relays all traffic between clients and agents, so agents behind CGNAT are reachable and clients never learn an agent's IP. Feasibility checked, nothing built yet.

- Control (HTTP signaling, settings, stats, end session): the agent opens a long-lived outbound connection (e.g. WebSocket) to the orchprox, configured manually on the agent. The orchprox forwards client requests for an agent down that connection. Custom code, but mostly request forwarding.
- Video and input (WebRTC): relayed by a standard TURN server (e.g. coturn) on the same VPS, not by custom code. Both ends get the TURN server in their ICE config (`webrtcbin` on the agent); the client forces relay with `iceTransportPolicy: "relay"`. The existing one-request signaling stays the same.
- Client: the address becomes orchprox + agent ID instead of an IP.
- Security: HTTPS on the orchprox (needs a domain), short-lived TURN credentials issued per session. Auth comes in v3.0.
- TURN over TLS on port 443 as a fallback for networks that block UDP.
- Costs: every stream crosses the VPS twice (about 9 GB each way per hour at 20 Mbps), so check the VPS transfer limits. The extra hop adds latency; pick a VPS region close to the usual users and agents.
- **TBD:** always relay (simple, predictable) or try a direct connection first to save VPS bandwidth. Direct often fails when both ends are behind CGNAT, so always relay is the starting point.
