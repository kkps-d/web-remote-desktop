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
  - Pipeline: `d3d11screencapturesrc ! d3d11convert ! <encoder bin> ! h264parse ! rtph264pay ! capssetter ! webrtcbin`.
  - Encoder: one of the H.264 picks in [ENCODERS.md](ENCODERS.md), chosen per session; the default is the first available (NVENC, AMF, Media Foundation, Direct3D 12, x264, Microsoft software).
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
- **v2.0:** reach agents over the internet through the orchprox.

### v0.5 scope

- Repo layout: `agent/` (C#), `client/` (React); `spikes/` kept for reference.
- Client:
  - Connect form for the agent address, pre-filled with the last used address (localStorage).
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
- Encoders the agent can't use are greyed out with the reason. GPU encoders count only on the GPU driving the captured display (found with the GStreamer device monitor); others would copy every frame between GPUs. Media Foundation encoders are off while NVENC is available, because running one breaks NVENC until the agent restarts (see [ENCODERS.md](ENCODERS.md)).
- Each encoder's size limits come from its sink pad template (NVENC, AMF and D3D12: 4096 px at most). A resolution or encoder change outside them is rejected with a 400 before the pipeline changes. A display larger than the default encoder accepts starts scaled down to fit.
- Agent: `GET /sessions/{id}` returns the source resolution, current settings and the encoder list; `PATCH /sessions/{id}` changes them while streaming. Defaults: native resolution, 30 fps, 8 Mbps.
- Agent: `GET /sessions/{id}/stats` returns cumulative encoding elapsed time and frame count. Encode time measures encoder input-to-output latency, including internal queueing.
- Changes apply live, without SDP renegotiation: new caps on the capsfilter after `d3d11convert` rescale and change the capture rate (the encoder restarts with a keyframe), and the encoder picks up a new `bitrate` on the next frame. Switching encoder blocks the capsfilter's output for one frame and swaps the encoder bin; the new encoder starts with a keyframe.

### v2.0 scope: orchprox

A server on a VPS with a public IP that coordinates the handshake and relays all traffic between clients and agents, so agents behind CGNAT are reachable and clients never learn an agent's IP. Feasibility checked, nothing built yet.

- Control (HTTP signaling, settings, stats, end session): the agent opens a long-lived outbound connection (e.g. WebSocket) to the orchprox, configured manually on the agent. The orchprox forwards client requests for an agent down that connection. Custom code, but mostly request forwarding.
- Video and input (WebRTC): relayed by a standard TURN server (e.g. coturn) on the same VPS, not by custom code. Both ends get the TURN server in their ICE config (`webrtcbin` on the agent); the client forces relay with `iceTransportPolicy: "relay"`. The existing one-request signaling stays the same.
- Client: the address becomes orchprox + agent ID instead of an IP.
- Security: agents register with a secret, clients log in, HTTPS on the orchprox (needs a domain), short-lived TURN credentials issued per session.
- TURN over TLS on port 443 as a fallback for networks that block UDP.
- Costs: every stream crosses the VPS twice (about 9 GB each way per hour at 20 Mbps), so check the VPS transfer limits. The extra hop adds latency; pick a VPS region close to the usual users and agents.
- **TBD:** always relay (simple, predictable) or try a direct connection first to save VPS bandwidth. Direct often fails when both ends are behind CGNAT, so always relay is the starting point.
