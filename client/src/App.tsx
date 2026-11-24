import { useEffect, useState } from "react"

import { ConnectScreen } from "@/components/connect-screen"
import { ViewerScreen } from "@/components/viewer-screen"
import { useAgentViewer } from "@/hooks/use-agent-viewer"
import { launchAgentFromSearch } from "@/lib/agent-connection"

export default function App() {
  const { state, connect, disconnect } = useAgentViewer()
  const [launch] = useState(() =>
    launchAgentFromSearch(window.location.search, window.location.hostname),
  )
  const [useAgentDefaults, setUseAgentDefaults] = useState(Boolean(launch))

  useEffect(() => {
    if (!launch) return
    // Defer until after Strict Mode finishes its initial effect cleanup.
    const timer = window.setTimeout(() => void connect(launch), 0)
    return () => window.clearTimeout(timer)
  }, [connect, launch])

  function connectManually(address: string) {
    setUseAgentDefaults(false)
    return connect(address)
  }

  if (state.status === "idle" || state.status === "ended" || state.status === "failed") {
    return (
      <ConnectScreen
        onConnect={connectManually}
        error={state.status === "failed" ? state.error : undefined}
      />
    )
  }

  return (
    <ViewerScreen
      address={state.address}
      connection={state.connection}
      status={state.status}
      onDisconnect={disconnect}
      useAgentDefaults={useAgentDefaults}
    />
  )
}
