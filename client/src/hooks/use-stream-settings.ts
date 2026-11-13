import { useCallback, useEffect, useState } from "react"

import {
  scaleToHeight,
  type AgentConnection,
  type Resolution,
  type StreamInfo,
  type StreamSettings,
} from "@/lib/agent-connection"

const SAVED_SETTINGS_KEY = "streamSettings"

// Resolution is saved as a height (null = native) so it adapts to each agent's source size.
interface SavedSettings {
  height: number | null
  framerate: number
  bitrateKbps: number
}

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

      const saved = readSavedSettings()
      if (!saved) return { info }
      try {
        return { info: await connection.updateStreamSettings(resolveSavedSettings(saved, info.source)) }
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
        saveSettings(info)
      } catch (error) {
        setState((previous) => ({ ...previous, connection, error: errorMessage(error) }))
      }
    },
    [connection],
  )

  return { info: current?.info, error: current?.error, update }
}

/** Turns saved settings into concrete ones for this source; heights at or above it mean native. */
function resolveSavedSettings(saved: SavedSettings, source: Resolution): StreamSettings {
  const size = saved.height === null || saved.height >= source.height ? source : scaleToHeight(source, saved.height)
  return { ...size, framerate: saved.framerate, bitrateKbps: saved.bitrateKbps }
}

function readSavedSettings(): SavedSettings | undefined {
  try {
    const saved = JSON.parse(localStorage.getItem(SAVED_SETTINGS_KEY) ?? "null")
    if (
      (saved?.height === null || Number.isInteger(saved?.height)) &&
      Number.isInteger(saved?.framerate) &&
      Number.isInteger(saved?.bitrateKbps)
    )
      return saved
  } catch {
    // A malformed saved value falls back to the agent's defaults.
  }
}

function saveSettings({ source, settings }: StreamInfo) {
  const saved: SavedSettings = {
    height: settings.height === source.height ? null : settings.height,
    framerate: settings.framerate,
    bitrateKbps: settings.bitrateKbps,
  }
  localStorage.setItem(SAVED_SETTINGS_KEY, JSON.stringify(saved))
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
