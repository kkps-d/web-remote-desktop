# Requirements

Running summary of requirement discussions. Open questions are marked **TBD**.

## Web client

- Browser-based WebRTC client that connects to the agent running on the remote computer.
- Built with React (+ TypeScript, Vite) and shadcn/ui, per the project guidelines.
  - React handles the UI shell: connect screen, connection status, toolbar, settings.
  - WebRTC (`RTCPeerConnection`) lives in a plain TypeScript module or a single hook, outside React's render cycle.
  - Remote screen is a `<video>` element fed the incoming `MediaStream` via a ref.
  - Mouse/keyboard input goes over an `RTCDataChannel` from native DOM listeners, not React state, to keep latency low.
- **TBD:** signaling approach (how client and agent exchange SDP offers and ICE candidates).
- **TBD:** NAT traversal needs (STUN only, or a TURN server too).
- **TBD:** authentication and how a client finds or pairs with an agent.
