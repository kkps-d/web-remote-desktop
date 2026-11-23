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

/** Reads the opt-in launch address for an agent on this computer. */
export function localAgentFromSearch(search: string): string | undefined {
  const address = new URLSearchParams(search).get("localAgent")
  if (!address) return
  const port = /^(?:127\.0\.0\.1|localhost):([1-9]\d{0,4})$/.exec(address)?.[1]
  if (port && Number(port) <= 65535) return address
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
  const baseUrl = parseAgentAddress(address)
  const peerConnection = new RTCPeerConnection()
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

    const response = await postOffer(baseUrl, peerConnection.localDescription!.sdp)
    const answer = await response.text()
    if (!response.ok) {
      throw new Error(answer || `The agent responded with ${response.status}.`)
    }

    const location = response.headers.get("Location")
    sessionUrl = location ? new URL(location, baseUrl) : null
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

async function postOffer(baseUrl: URL, sdp: string): Promise<Response> {
  try {
    return await fetch(new URL("/offer", baseUrl), {
      method: "POST",
      headers: { "Content-Type": "application/sdp" },
      body: sdp,
    })
  } catch {
    throw new Error(
      `Couldn't reach the agent at ${baseUrl.host}. Check the address and that the agent is running.`,
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
