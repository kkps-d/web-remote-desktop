import { useEffect, useRef, useState, type ReactNode } from "react"
import { Activity, LoaderCircle, Maximize, Minimize, Unplug } from "lucide-react"

import { StatsOverlay } from "@/components/stats-overlay"
import { StreamSettingsPicker } from "@/components/stream-settings-picker"
import { useStreamSettings } from "@/hooks/use-stream-settings"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { AgentConnection } from "@/lib/agent-connection"

interface ViewerScreenProps {
  address: string
  connection?: AgentConnection
  status: "connecting" | "connected" | "reconnecting"
  onDisconnect: () => void
}

const statusLabels = { connecting: "Connecting…", connected: "Connected", reconnecting: "Reconnecting…" }

export function ViewerScreen({ address, connection, status, onDisconnect }: ViewerScreenProps) {
  const connected = status === "connected"
  const stageRef = useRef<HTMLDivElement>(null)
  const videoRef = useRef<HTMLVideoElement>(null)
  const [showStats, setShowStats] = useState(true)
  const isFullscreen = useIsFullscreen()
  const streamSettings = useStreamSettings(connection)

  useEffect(() => {
    if (videoRef.current) videoRef.current.srcObject = connection?.stream ?? null
  }, [connection])

  function toggleFullscreen() {
    if (document.fullscreenElement) document.exitFullscreen()
    else stageRef.current?.requestFullscreen()
  }

  return (
    <div className="flex h-svh flex-col bg-background">
      <header className="flex items-center gap-2 border-b px-3 py-2">
        <Badge variant={connected ? "default" : "secondary"}>{statusLabels[status]}</Badge>
        <span className="min-w-0 truncate text-sm text-muted-foreground">{address}</span>

        <div className="ml-auto flex items-center gap-1">
          <StreamSettingsPicker
            info={streamSettings.info}
            error={streamSettings.error}
            disabled={!connected}
            onChange={streamSettings.update}
          />
          <ToolbarButton
            label={showStats ? "Hide stats" : "Show stats"}
            onClick={() => setShowStats((shown) => !shown)}
            pressed={showStats}
          >
            <Activity />
          </ToolbarButton>
          <ToolbarButton label={isFullscreen ? "Exit fullscreen" : "Fullscreen"} onClick={toggleFullscreen}>
            {isFullscreen ? <Minimize /> : <Maximize />}
          </ToolbarButton>
          <Button variant="outline" onClick={onDisconnect}>
            <Unplug aria-hidden />
            Disconnect
          </Button>
        </div>
      </header>

      <div ref={stageRef} className="relative min-h-0 flex-1 bg-black">
        <video
          ref={videoRef}
          className="size-full object-contain"
          aria-label={`Remote desktop of ${address}`}
          autoPlay
          muted
          playsInline
        />
        {!connected && (
          // The dimmed backdrop keeps the text readable over the last frame of a lost stream.
          <div
            role="status"
            className="absolute inset-0 flex items-center justify-center gap-2 bg-black/60 text-sm text-white"
          >
            <LoaderCircle className="size-4 animate-spin motion-reduce:animate-none" aria-hidden />
            {status === "reconnecting" ? "Connection lost. Reconnecting…" : `Connecting to ${address}…`}
          </div>
        )}
        {showStats && connected && connection && (
          <StatsOverlay peerConnection={connection.peerConnection} source={streamSettings.info?.source} />
        )}
      </div>
    </div>
  )
}

function ToolbarButton({
  label,
  onClick,
  pressed,
  children,
}: {
  label: string
  onClick: () => void
  pressed?: boolean
  children: ReactNode
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={<Button variant="ghost" size="icon" aria-label={label} aria-pressed={pressed} onClick={onClick} />}
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

function useIsFullscreen() {
  const [isFullscreen, setIsFullscreen] = useState(() => document.fullscreenElement != null)

  useEffect(() => {
    const update = () => setIsFullscreen(document.fullscreenElement != null)
    document.addEventListener("fullscreenchange", update)
    return () => document.removeEventListener("fullscreenchange", update)
  }, [])

  return isFullscreen
}
