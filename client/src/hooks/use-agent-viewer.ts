import { useCallback, useEffect, useRef, useState } from "react"

import { connectToAgent, type AgentConnection } from "@/lib/agent-connection"

export type ViewerState =
  | { status: "idle" }
  | { status: "ended" }
  | { status: "connecting"; address: string; connection?: AgentConnection }
  | { status: "connected"; address: string; connection: AgentConnection }
  | { status: "reconnecting"; address: string; connection: AgentConnection }
  | { status: "failed"; address: string; error: string }

/** Manages the lifecycle of one viewer connection to an agent. */
export function useAgentViewer(openConnection = connectToAgent) {
  const [state, setState] = useState<ViewerState>({ status: "idle" })
  const connectionRef = useRef<AgentConnection | null>(null)
  // Aborted on every connect/disconnect so a superseded attempt stops and its results are ignored.
  const attemptRef = useRef<AbortController | null>(null)

  const closeConnection = useCallback(() => {
    attemptRef.current?.abort()
    attemptRef.current = null
    connectionRef.current?.close()
    connectionRef.current = null
  }, [])

  const connect = useCallback(
    async (address: string) => {
      closeConnection()
      const attempt = new AbortController()
      attemptRef.current = attempt
      setState({ status: "connecting", address })

      let connection: AgentConnection
      try {
        connection = await openConnection(address, attempt.signal)
      } catch (error) {
        if (attempt.signal.aborted) return
        setState({
          status: "failed",
          address,
          error: error instanceof Error ? error.message : String(error),
        })
        return
      }

      if (attempt.signal.aborted) {
        connection.close()
        return
      }
      connectionRef.current = connection
      setState({ status: "connecting", address, connection })

      const { peerConnection } = connection
      const updateConnectionState = () => {
        if (attempt.signal.aborted) return
        switch (peerConnection.connectionState) {
          case "connected":
            setState({ status: "connected", address, connection })
            break
          // The browser keeps trying to recover until it gives up with "failed".
          case "disconnected":
            setState({ status: "reconnecting", address, connection })
            break
          case "failed":
            closeConnection()
            setState({ status: "failed", address, error: "The connection to the agent was lost." })
            break
        }
      }
      peerConnection.onconnectionstatechange = updateConnectionState
      updateConnectionState()

      if (connection.isGameAvailable && !attempt.signal.aborted) {
        const isGameAvailable = connection.isGameAvailable
        let statusTimer = window.setTimeout(checkStatus, 1500)
        attempt.signal.addEventListener("abort", () => window.clearTimeout(statusTimer), {
          once: true,
        })

        async function checkStatus() {
          const available = await isGameAvailable(attempt.signal).catch(() => true)
          if (attempt.signal.aborted) return
          if (!available) {
            closeConnection()
            setState({ status: "ended" })
          } else {
            statusTimer = window.setTimeout(checkStatus, 1500)
          }
        }
      }
    },
    [closeConnection, openConnection],
  )

  const disconnect = useCallback(() => {
    closeConnection()
    setState({ status: "idle" })
  }, [closeConnection])

  // End the agent's session when the tab closes or the component unmounts. Resetting the state on
  // pagehide makes a page restored from the back/forward cache show the connect screen.
  useEffect(() => {
    window.addEventListener("pagehide", disconnect)
    return () => {
      window.removeEventListener("pagehide", disconnect)
      closeConnection()
    }
  }, [closeConnection, disconnect])

  return { state, connect, disconnect }
}
