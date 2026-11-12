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
  - Bindings: `gstreamer-sharp`.
  - Pipeline: `d3d11screencapturesrc ! d3d11convert ! mfh264enc ! h264parse ! rtph264pay ! webrtcbin`.
  - Encoder: `mfh264enc` (uses the hardware encoder when available), `x264enc tune=zerolatency` as fallback.
- Signaling: ASP.NET Core minimal API, `POST /offer` with CORS.
  - The browser is the offerer (receive-only video + `input` data channel).
  - The agent passes the offer to `webrtcbin`, waits for ICE gathering to complete, and returns the answer.
- Input: data channel messages (via `webrtcbin`'s `on-data-channel`) are translated into `SendInput` calls via P/Invoke.
- Main risk: `gstreamer-sharp` maturity with `webrtcbin` on Windows. De-risk with a spike:
  1. Verify capture + encode with `gst-launch-1.0` (official GStreamer MSVC build).
  2. Build and run the same pipeline from a minimal C# app.
  3. Complete one offer/answer exchange with the browser through `webrtcbin`.
- Fallback if `webrtcbin` from C# is too painful: keep GStreamer for capture/encoding, use SIPSorcery (pure C# WebRTC) for WebRTC.
- Out of scope for now: Windows service, login screen/UAC capture, audio, multiple monitors.

## Milestones

- **v0.5:** capture the desktop on the agent and stream it to the web client. View only, no input.
- **v1:** v0.5 plus mouse and keyboard input over the data channel.

<sub>Future: a separate auth + UDP proxy server so clients never learn the agent's IP.</sub>
