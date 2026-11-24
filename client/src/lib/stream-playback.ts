// Starts the remote stream, falling back to muted when the browser blocks audio.

import { parseAgentAddress } from "./agent-connection.ts"

type PlayableVideo = Pick<HTMLVideoElement, "muted" | "play">

/**
 * Plays the video. Browsers block unmuted playback until the user has interacted with the page,
 * which would stop the video too, so a blocked play is retried muted.
 * Resolves to true if it had to mute.
 */
export async function playWithMutedFallback(
  video: PlayableVideo,
  isCurrent = () => true,
): Promise<boolean> {
  try {
    await video.play()
    return false
  } catch (error) {
    // Other failures, such as AbortError when the stream is replaced, aren't about audio.
    if (!isCurrent() || video.muted || (error as DOMException).name !== "NotAllowedError")
      return false
    video.muted = true
    await video.play().catch(() => {})
    return true
  }
}

/**
 * Whether the agent runs on this computer. Its audio capture would pick up our playback and
 * send it back, so such sessions start muted.
 */
export function isLocalAgent(address: string): boolean {
  const { hostname } = parseAgentAddress(address)
  return hostname === "localhost" || hostname === "[::1]" || hostname.startsWith("127.")
}
