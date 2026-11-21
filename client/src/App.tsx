import { ConnectScreen } from "@/components/connect-screen"
import { ViewerScreen } from "@/components/viewer-screen"
import { useAgentViewer } from "@/hooks/use-agent-viewer"

export default function App() {
  const { state, connect, disconnect } = useAgentViewer()

  if (state.status === "idle" || state.status === "failed") {
    return (
      <ConnectScreen
        onConnect={connect}
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
    />
  )
}
