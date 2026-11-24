import { useCallback, useEffect, useRef, useState } from "react"

import type { AgentConnection, StreamInfo } from "@/lib/agent-connection"
import { defaultGameQuality, fitGameQuality, matchesGameQuality } from "@/lib/game-quality"

interface QualityState {
  connection: AgentConnection
  initial?: StreamInfo
  info?: StreamInfo
  applied?: number
  pendingPreset?: number
  error?: string
}

interface QualityRequest {
  signal: AbortSignal
  busy: boolean
}

export function useGameQuality(connection: AgentConnection | undefined) {
  const [state, setState] = useState<QualityState>()
  const current = state?.connection === connection ? state : undefined
  const preferred = useRef(defaultGameQuality)
  const session = useRef<QualityRequest>(null)

  const apply = useCallback(
    async (previous: QualityState, preset: number, request: QualityRequest) => {
      if (!previous.initial || !previous.info || request.busy || request.signal.aborted) return

      const target = fitGameQuality(preset, previous.initial)
      request.busy = true
      setState({ ...previous, pendingPreset: preset, error: undefined })
      try {
        const info = matchesGameQuality(previous.info.settings, target)
          ? previous.info
          : await previous.connection.updateStreamSettings(target)
        if (request.signal.aborted) return
        preferred.current = preset
        setState({ ...previous, info, applied: preset, pendingPreset: undefined, error: undefined })
      } catch {
        if (request.signal.aborted) return
        setState({
          ...previous,
          pendingPreset: undefined,
          error: "Couldn't apply the stream quality.",
        })
      } finally {
        request.busy = false
      }
    },
    [],
  )

  useEffect(() => {
    if (!connection) return
    const controller = new AbortController()
    const request = { signal: controller.signal, busy: false }
    session.current = request
    connection.getStreamInfo().then(
      (info) => {
        if (request.signal.aborted) return
        void apply({ connection, initial: info, info }, preferred.current, request)
      },
      () => {
        if (!request.signal.aborted) {
          setState({ connection, error: "Couldn't load the stream limits." })
        }
      },
    )
    return () => controller.abort()
  }, [apply, connection])

  return {
    info: current?.info,
    applied: current?.applied,
    error: current?.error,
    pendingPreset: current?.pendingPreset,
    select(preset: number) {
      if (current && session.current) void apply(current, preset, session.current)
    },
  }
}
