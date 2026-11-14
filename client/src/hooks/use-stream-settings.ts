import { useCallback, useEffect, useState } from "react"

import type { AgentConnection, StreamInfo, StreamSettings } from "@/lib/agent-connection"
import { readStreamSettings, resolveSavedSettings, saveStreamSettings } from "@/lib/saved-settings"

// Tagging state with its connection means a new connection starts empty without resetting state in an effect.
interface State {
  connection: AgentConnection
  info?: StreamInfo
  error?: string
}

/** Loads a connection's stream info, applies the last saved settings and saves changes. */
export function useStreamSettings(connection?: AgentConnection) {
  const [state, setState] = useState<State>()
  const current = state?.connection === connection ? state : undefined

  useEffect(() => {
    if (!connection) return
    let cancelled = false

    async function load(connection: AgentConnection): Promise<Omit<State, "connection">> {
      let info: StreamInfo
      try {
        info = await connection.getStreamInfo()
      } catch (error) {
        return { error: errorMessage(error) }
      }

      const saved = readStreamSettings()
      if (!saved) return { info }
      try {
        return { info: await connection.updateStreamSettings(resolveSavedSettings(saved, info)) }
      } catch (error) {
        // Keep streaming with the agent's defaults; the picker shows why.
        return { info, error: errorMessage(error) }
      }
    }

    load(connection).then((result) => !cancelled && setState({ connection, ...result }))
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
        saveStreamSettings(info)
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
