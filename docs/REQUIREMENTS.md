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
  - While on HTTP, the client must also be served over HTTP (or `localhost`), because HTTPS pages block requests to HTTP addresses.
  - Known HTTP limitations: signaling can be tampered with on the network, and secure-context APIs (clipboard, Keyboard Lock) only work when the client runs on `localhost`.

## Agent

- Windows only for now; keep capture and input code isolated so other OSes can be added later.
- C# (.NET 10) console app running in the logged-in user's session (not a Windows service).
- GStreamer does capture, encoding and WebRTC; C# only wires it together.
  - Bindings: `gstreamer-sharp-netcore` 0.0.8 (the only `gstreamer-sharp` package on NuGet).
  - Pipeline: `d3d11screencapturesrc ! d3d11convert ! mfh264enc ! h264parse ! rtph264pay ! capssetter ! webrtcbin`.
  - Encoder: `mfh264enc` (uses the hardware encoder when available), `x264enc tune=zerolatency` as fallback.
- Signaling: ASP.NET Core minimal API, `POST /offer` with CORS.
  - The browser is the offerer (receive-only video + `input` data channel).
  - The agent passes the offer to `webrtcbin`, waits for ICE gathering to complete, and returns the answer.
- Input: data channel messages (via `webrtcbin`'s `on-data-channel`) are translated into `SendInput` calls via P/Invoke.
- Fallback if `webrtcbin` from C# becomes too painful: keep GStreamer for capture/encoding, use SIPSorcery (pure C# WebRTC) for WebRTC.

### Spike results (`spikes/GstSpike`)

The C# + GStreamer + `webrtcbin` approach works: the desktop (2560×1440) streams to Chrome via one HTTP POST, encoded on the GPU by `mfh264enc`. Requires GStreamer 1.28 (official MSVC build, "Complete" install). Workarounds needed:

- The per-user GStreamer install isn't on `PATH`; the agent prepends its `bin` folder at startup.
- GLibSharp looks for MinGW DLL names (`libgobject-2.0-0.dll`); the agent pre-fills its private handle cache with the MSVC DLLs.
- The bindings can't pass `null` signal arguments (use an empty `Structure`), and reading `local-description` frees the SDP too early (read it via `g_object_get` P/Invoke).
- `webrtcbin` as answerer matches our stream to the offer by exact RTP caps:
  - wait for caps on its sink pad before setting the remote description;
  - use the browser's payload type for constrained-baseline H.264;
  - relabel `profile-level-id` to the browser's via `capssetter` (desktop resolutions need a higher level; browsers allow it with `level-asymmetry-allowed=1`).
- Out of scope for now: Windows service, login screen/UAC capture, audio, multiple monitors.

## Milestones

- **v0.5:** capture the desktop on the agent and stream it to the web client. View only, no input.
- **v1:** v0.5 plus mouse and keyboard input over the data channel.

### v0.5 scope

- Repo layout: `agent/` (C#), `client/` (React); `spikes/` kept for reference.
- Client:
  - Connect form for the agent address, pre-filled with the last used address (localStorage).
  - Connection status: connecting, connected, failed (with the reason).
  - Stream view scaled to fit the window, keeping aspect ratio.
  - Fullscreen and Disconnect buttons.
  - Stats overlay: resolution, fps, bitrate.
- Agent:
  - Listens on all interfaces on a default port (8080), overridable via command-line argument or `appsettings.json`.
  - One viewer at a time: a new connection replaces the current viewer.
  - Tears down the pipeline when the viewer disconnects (client `DELETE` on the session, WHEP-style) or the WebRTC connection fails.
- Out of scope for v0.5: input, auth, HTTPS.

### Stream settings

- Client: a toolbar picker for resolution (native or smaller, same aspect ratio), frame rate and bitrate; the stats overlay shows source and stream resolution.
- Agent: `GET /sessions/{id}` returns the source resolution and current settings; `PATCH /sessions/{id}` changes them while streaming. Defaults: native resolution, 30 fps, 8 Mbps.
- Changes apply live, without SDP renegotiation: new caps on the capsfilter after `d3d11convert` rescale and change the capture rate (the encoder restarts with a keyframe), and `mfh264enc` picks up a new `bitrate` on the next frame.

<sub>Future: a separate auth + UDP proxy server so clients never learn the agent's IP.</sub>
