import { useCallback, useEffect, useState } from "react"

import type { AgentConnection, StreamInfo, StreamSettings } from "@/lib/agent-connection"
import { readStreamSettings, resolveSavedSettings, saveStreamSettings } from "@/lib/saved-settings"

// Tagging state with its connection means a new connection starts empty without resetting state in an effect.
interface State {
  connection: AgentConnection
  info?: StreamInfo
  error?: string
}

/** Uses the agent defaults for a local launch. Manual connections remember stream preferences. */
export function useStreamSettings(
  connection: AgentConnection | undefined,
  useAgentDefaults: boolean,
) {
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

      const saved = useAgentDefaults ? undefined : readStreamSettings()
      if (!saved) return { info }
      try {
        return { info: await connection.updateStreamSettings(resolveSavedSettings(saved, info)) }
      } catch (error) {
        // Keep streaming with the agent's defaults; the picker shows why.
        return { info, error: errorMessage(error) }
      }
    }

    const refresh = () => {
      connection.getStreamInfo().then(
        (info) => {
          if (!cancelled) setState((previous) => ({ ...previous, connection, info }))
        },
        () => {}, // The session may already have ended.
      )
    }
    connection.input.addEventListener("close", refresh)
    load(connection).then((result) => !cancelled && setState({ connection, ...result }))
    return () => {
      cancelled = true
      connection.input.removeEventListener("close", refresh)
    }
  }, [connection, useAgentDefaults])

  const update = useCallback(
    async (change: Partial<StreamSettings>) => {
      if (!connection) return
      try {
        const info = await connection.updateStreamSettings(change)
        setState({ connection, info })
        if (!useAgentDefaults) saveStreamSettings(info)
      } catch (error) {
        setState((previous) => ({ ...previous, connection, error: errorMessage(error) }))
      }
    },
    [connection, useAgentDefaults],
  )

  return { info: current?.info, error: current?.error, update }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
