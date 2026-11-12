import { useCallback, useEffect, useState } from "react"

import type { AgentConnection, StreamInfo, StreamSettings } from "@/lib/agent-connection"

// Tagging state with its connection means a new connection starts empty without resetting state in an effect.
interface State {
  connection: AgentConnection
  info?: StreamInfo
  error?: string
}

/** Loads a connection's stream info and changes its settings. */
export function useStreamSettings(connection?: AgentConnection) {
  const [state, setState] = useState<State>()
  const current = state?.connection === connection ? state : undefined

  useEffect(() => {
    if (!connection) return
    let cancelled = false
    connection.getStreamInfo().then(
      (info) => !cancelled && setState({ connection, info }),
      (error) => !cancelled && setState({ connection, error: errorMessage(error) }),
    )
    return () => {
      cancelled = true
    }
  }, [connection])

  const update = useCallback(
    async (change: Partial<StreamSettings>) => {
      if (!connection) return
      try {
        const info = await connection.updateStreamSettings(change)
        setState({ connection, info })
      } catch (error) {
        setState((previous) => ({ ...previous, connection, error: errorMessage(error) }))
      }
    },
    [connection],
  )

  return { info: current?.info, error: current?.error, update }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
