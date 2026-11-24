import { useCallback, useEffect, useRef, useState, type RefObject } from "react"

import type { AgentConnection } from "@/lib/agent-connection"
import { playWithMutedFallback } from "@/lib/stream-playback"

export function useGamePlayback(
  videoRef: RefObject<HTMLVideoElement | null>,
  connection?: AgentConnection,
) {
  const attempt = useRef(0)
  const [needsAudioGesture, setNeedsAudioGesture] = useState(false)

  const play = useCallback((video: HTMLVideoElement) => {
    const id = ++attempt.current
    const current = () => id === attempt.current
    video.muted = false
    void playWithMutedFallback(video, current).then((blocked) => {
      if (current()) setNeedsAudioGesture(blocked)
    })
  }, [])

  const cancelPlayback = useCallback(() => {
    ++attempt.current
  }, [])

  useEffect(() => {
    const video = videoRef.current
    if (!connection || !video) return
    video.srcObject = connection.stream
    play(video)
    return () => {
      cancelPlayback()
      video.srcObject = null
    }
  }, [cancelPlayback, connection, play, videoRef])

  return {
    needsAudioGesture,
    enableAudio() {
      const video = videoRef.current
      if (video?.srcObject && (video.muted || video.paused)) play(video)
    },
  }
}
