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

<sub>Future: a separate auth + UDP proxy server so clients never learn the agent's IP.</sub>
