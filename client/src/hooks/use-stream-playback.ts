import { useEffect, useState, type RefObject } from "react"

import type { AgentConnection } from "@/lib/agent-connection"
import { readAudioMuted, saveAudioMuted } from "@/lib/saved-settings"
import { isLocalAgent, playWithMutedFallback } from "@/lib/stream-playback"

/**
 * Plays the connection's stream in the video, muted if the user muted it last time, the agent
 * runs on this computer, or the browser blocked audio. Returns whether audio is muted and a
 * toggle for the toolbar.
 */
export function useStreamPlayback(
  videoRef: RefObject<HTMLVideoElement | null>,
  address: string,
  connection?: AgentConnection,
) {
  // A local agent starts muted without saving it, so remote sessions keep the saved choice.
  const [userMuted, setUserMuted] = useState(() => readAudioMuted() || isLocalAgent(address))
  // Set when the browser blocked unmuted playback; it isn't saved as the user's choice.
  const [blocked, setBlocked] = useState(false)
  const muted = userMuted || blocked

  // Declared before the play effect so a saved mute applies before playback starts.
  useEffect(() => {
    if (videoRef.current) videoRef.current.muted = muted
  }, [videoRef, muted])

  useEffect(() => {
    const video = videoRef.current
    if (!video) return
    video.srcObject = connection?.stream ?? null
    if (!connection) return
    let active = true
    playWithMutedFallback(video).then((mutedByBrowser) => {
      if (mutedByBrowser && active) setBlocked(true)
    })
    return () => {
      active = false
    }
  }, [videoRef, connection])

  function toggleMuted() {
    const next = !muted
    setUserMuted(next)
    setBlocked(false)
    saveAudioMuted(next)
    // Unmute within the click, which counts as the interaction the browser waits for.
    const video = videoRef.current
    if (video) {
      video.muted = next
      if (!next) video.play().catch(() => {})
    }
  }

  return { muted, toggleMuted }
}
