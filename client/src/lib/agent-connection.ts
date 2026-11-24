// WebRTC connection to an agent. Plain TypeScript, kept outside React's render cycle.

const DEFAULT_AGENT_PORT = 8080
const ICE_GATHERING_TIMEOUT_MS = 5000

export interface AgentConnection {
  peerConnection: RTCPeerConnection
  /** Receives the remote desktop video track once media starts flowing. */
  stream: MediaStream
  /** Reliable, ordered channel for mouse and keyboard input (see `input-sender.ts`). */
  input: RTCDataChannel
  /** Fetches the source resolution and current stream settings. */
  getStreamInfo: () => Promise<StreamInfo>
  getEncodingStats: (signal?: AbortSignal) => Promise<EncodingStats>
  /** Changes stream settings while streaming; omitted fields keep their value. */
  updateStreamSettings: (update: Partial<StreamSettings>) => Promise<StreamInfo>
  /** Closes the connection and tells the agent to stop streaming. */
  close: () => void
  isGameAvailable?: (signal: AbortSignal) => Promise<boolean>
}

export interface Resolution {
  width: number
  height: number
}

/** The source scaled to the given height, keeping its aspect ratio. */
export function scaleToHeight(source: Resolution, height: number): Resolution {
  // The encoder needs even dimensions.
  return { width: Math.round((height * source.width) / source.height / 2) * 2, height }
}

export interface EncodingStats {
  totalEncodeTimeMs: number
  framesEncoded: number
}

export interface StreamSettings extends Resolution {
  framerate: number
  bitrateKbps: number
  /** Id of an encoder from `StreamInfo.encoders`. */
  encoder: string
}

/** An H.264 encoder on the agent; unavailable ones can't run on that computer. */
export interface EncoderOption {
  id: string
  label: string
  available: boolean
  unavailableReason?: string | null
}

export interface StreamInfo {
  source: Resolution
  settings: StreamSettings
  encoders: EncoderOption[]
  limits: StreamLimits
  /** Why the stream has no audio, or null if it has. */
  audioUnavailableReason: string | null
  inputUnavailableReason?: string | null
  touchSupported?: boolean
}

/** The most the agent allows. `resolution` is the source, or scaled down to the agent's limit. */
export interface StreamLimits {
  resolution: Resolution
  framerate: number
  bitrateKbps: number
}

/** Launch links can target loopback or the host that served the viewer. */
export function launchAgentFromSearch(search: string, hostname: string): string | undefined {
  const params = new URLSearchParams(search)
  const local = /^(?:127\.0\.0\.1|localhost):([1-9]\d{0,4})$/.exec(params.get("localAgent") ?? "")
  if (local && Number(local[1]) <= 65535) return local[0]

  const port = params.get("agentPort")
  if (hostname && port && /^[1-9]\d{0,4}$/.test(port) && Number(port) <= 65535) {
    return `${hostname}:${port}`
  }
}

export function launchGameFromSearch(search: string): string | undefined {
  const instance = new URLSearchParams(search).get("instance")
  if (instance && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(instance)) return instance
}

export async function connectToGame(
  instanceId: string,
  signal: AbortSignal,
): Promise<AgentConnection> {
  const base = new URL(
    `/api/games/instances/${encodeURIComponent(instanceId)}/stream/`,
    window.location.origin,
  )
  const response = await fetch(new URL("ice", base), { signal })
  if (!response.ok) {
    const body = await response.json().catch(() => null)
    const message = typeof body?.error === "string" ? body.error : "Couldn't open the game stream."
    throw new Error(message)
  }
  const { iceServers } = (await response.json()) as { iceServers: RTCIceServer[] }
  signal.throwIfAborted()
  const connection = await connectViewer(new URL("offer", base), signal, {
    iceServers,
    iceTransportPolicy: "relay",
  })
  return {
    ...connection,
    async isGameAvailable(signal) {
      const status = await fetch(new URL("status", base), { signal })
      return ![401, 404, 410].includes(status.status)
    },
  }
}

/** Turns user input like "192.168.1.20" or "pc.local:9000" into the agent's base URL. */
export function parseAgentAddress(address: string): URL {
  const trimmed = address.trim()
  const withScheme = /^https?:\/\//.test(trimmed) ? trimmed : `http://${trimmed}`
  const url = new URL(withScheme)
  // URL drops a port equal to the scheme's default (":80" for http), so check what was typed.
  const authority = withScheme.replace(/^https?:\/\//, "").split(/[/?#]/)[0]
  if (!/:\d+$/.test(authority)) url.port = String(DEFAULT_AGENT_PORT)
  return url
}

/**
 * Connects to the agent with WHEP-style signaling: one POST with the complete offer
 * (non-trickle ICE), answered with the agent's SDP and a session URL for ending it.
 * Aborting `signal` cancels the attempt and ends any session it already started.
 */
export async function connectToAgent(
  address: string,
  signal: AbortSignal,
): Promise<AgentConnection> {
  return connectViewer(new URL("/offer", parseAgentAddress(address)), signal)
}

async function connectViewer(
  offerUrl: URL,
  signal: AbortSignal,
  configuration?: RTCConfiguration,
): Promise<AgentConnection> {
  const peerConnection = new RTCPeerConnection(configuration)
  const stream = new MediaStream()
  let sessionUrl: URL | null = null

  peerConnection.addTransceiver("video", { direction: "recvonly" })
  // SPIKE: after the video, so it matches the agent's second webrtcbin pad.
  peerConnection.addTransceiver("audio", { direction: "recvonly" })
  peerConnection.ontrack = (event) => stream.addTrack(event.track)
  // Created before the offer so the SDP negotiates the data channel along with the video.
  const input = peerConnection.createDataChannel("input")

  try {
    await peerConnection.setLocalDescription(await peerConnection.createOffer())
    await waitForIceGathering(peerConnection)
    // A new session would replace whichever viewer the agent has now, so don't start one if cancelled.
    signal.throwIfAborted()

    const response = await postOffer(offerUrl, peerConnection.localDescription!.sdp)
    const answer = await response.text()
    if (!response.ok) {
      throw new Error(answer || `The agent responded with ${response.status}.`)
    }

    const location = response.headers.get("Location")
    sessionUrl = location ? new URL(location, offerUrl) : null
    await peerConnection.setRemoteDescription({ type: "answer", sdp: answer })
    signal.throwIfAborted()

    // Concurrent requests can reach the agent and resolve out of order, so an older reply could
    // overwrite a newer setting. Sending one at a time keeps both in the order the user chose.
    let lastRequest: Promise<unknown> = Promise.resolve()
    const queuedSessionRequest = (init: RequestInit) => {
      const request = lastRequest.then(() => sessionRequest(sessionUrl, init))
      lastRequest = request.catch(() => {})
      return request
    }

    return {
      peerConnection,
      stream,
      input,
      getStreamInfo: () => queuedSessionRequest({ method: "GET" }),
      getEncodingStats: (signal) =>
        sessionRequest<EncodingStats>(sessionUrl ? new URL(`${sessionUrl.href}/stats`) : null, {
          signal,
        }),
      updateStreamSettings: (update) =>
        queuedSessionRequest({
          method: "PATCH",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(update),
        }),
      close: () => {
        peerConnection.close()
        endSession(sessionUrl)
      },
    }
  } catch (error) {
    peerConnection.close()
    // The agent may already be streaming to us.
    endSession(sessionUrl)
    throw error
  }
}

async function sessionRequest<T = StreamInfo>(
  sessionUrl: URL | null,
  init: RequestInit,
): Promise<T> {
  if (!sessionUrl) throw new Error("The agent didn't return a session URL.")
  let response: Response
  try {
    response = await fetch(sessionUrl, init)
  } catch {
    throw new Error("Couldn't reach the agent.")
  }
  if (response.status === 404) throw new Error("The stream session has ended.")
  if (!response.ok)
    throw new Error((await response.text()) || `The agent responded with ${response.status}.`)
  return response.json()
}

function endSession(sessionUrl: URL | null) {
  // keepalive lets the request finish even when the page is being closed.
  if (sessionUrl) fetch(sessionUrl, { method: "DELETE", keepalive: true }).catch(() => {})
}

async function postOffer(offerUrl: URL, sdp: string): Promise<Response> {
  try {
    return await fetch(offerUrl, {
      method: "POST",
      headers: { "Content-Type": "application/sdp" },
      body: sdp,
    })
  } catch {
    throw new Error(
      `Couldn't reach the agent at ${offerUrl.host}. Check the address and that the agent is running.`,
    )
  }
}

function waitForIceGathering(peerConnection: RTCPeerConnection): Promise<void> {
  return new Promise((resolve) => {
    if (peerConnection.iceGatheringState === "complete") return resolve()

    // Send whatever candidates we have if gathering stalls; host candidates are enough on a LAN.
    const timeout = setTimeout(done, ICE_GATHERING_TIMEOUT_MS)
    peerConnection.addEventListener("icegatheringstatechange", () => {
      if (peerConnection.iceGatheringState === "complete") done()
    })

    function done() {
      clearTimeout(timeout)
      resolve()
    }
  })
}
