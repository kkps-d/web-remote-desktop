import { useCallback, useEffect, useRef, useState, type ReactNode } from "react"
import { LoaderCircle, Maximize, Minimize, Unplug } from "lucide-react"

import { InputMenu } from "@/components/input-menu"
import { ShortcutsMenu } from "@/components/shortcuts-menu"
import { StatsOverlay } from "@/components/stats-overlay"
import { StatsMenu } from "@/components/stats-menu"
import { StreamSettingsPicker } from "@/components/stream-settings-picker"
import { useInputSettings, useRemoteInput } from "@/hooks/use-remote-input"
import { useStreamSettings } from "@/hooks/use-stream-settings"
import { LEAVE_SHORTCUT } from "@/lib/keyboard-input"
import { captureThumbnail, rememberDevice } from "@/lib/recent-devices"
import { readStorage, writeStorage } from "@/lib/storage"
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

const STATS_VISIBILITY_KEY = "statsVisibility"
// The first capture waits a moment so the stream has settled past its first frames.
const FIRST_THUMBNAIL_DELAY_MS = 2000
const THUMBNAIL_INTERVAL_MS = 60_000

interface StatsVisibility {
  basic: boolean
  advanced: boolean
}

export function ViewerScreen({ address, connection, status, onDisconnect }: ViewerScreenProps) {
  const connected = status === "connected"
  const stageRef = useRef<HTMLDivElement>(null)
  const toolbarRef = useRef<HTMLDivElement>(null)
  const videoRef = useRef<HTMLVideoElement>(null)
  const [statsVisibility, setStatsVisibility] = useStatsVisibility()
  const isFullscreen = useIsFullscreen()
  const streamSettings = useStreamSettings(connection)
  const [inputSettings, setInputSettings] = useInputSettings()
  // The toolbar is hidden in fullscreen, so leaving the stage leaves fullscreen too.
  const focusToolbar = useCallback(() => {
    if (document.fullscreenElement) document.exitFullscreen()
    toolbarRef.current?.querySelector("button")?.focus()
  }, [])
  const inputSender = useRemoteInput({
    connection,
    stageRef,
    videoRef,
    onLeaveKeyboard: focusToolbar,
    ...inputSettings,
  })

  useEffect(() => {
    if (videoRef.current) videoRef.current.srcObject = connection?.stream ?? null
  }, [connection])

  // Remember the device for the connect screen, refreshing its thumbnail so one survives a closed tab.
  useEffect(() => {
    const video = videoRef.current
    if (!connected || !video) return
    rememberDevice(address)
    const saveThumbnail = () => rememberDevice(address, captureThumbnail(video))
    let interval: ReturnType<typeof setInterval> | undefined
    const firstCapture = setTimeout(() => {
      saveThumbnail()
      interval = setInterval(saveThumbnail, THUMBNAIL_INTERVAL_MS)
    }, FIRST_THUMBNAIL_DELAY_MS)
    return () => {
      clearTimeout(firstCapture)
      clearInterval(interval)
    }
  }, [connected, address])

  function disconnect() {
    // Store what the user last saw.
    if (connected && videoRef.current) rememberDevice(address, captureThumbnail(videoRef.current))
    onDisconnect()
  }

  function toggleFullscreen() {
    if (document.fullscreenElement) document.exitFullscreen()
    else stageRef.current?.requestFullscreen()
  }

  return (
    <div className="flex h-svh flex-col bg-background">
      <header className="flex items-center gap-2 border-b px-3 py-2">
        <Badge variant={connected ? "default" : "secondary"}>{statusLabels[status]}</Badge>
        <span className="min-w-0 truncate text-sm text-muted-foreground">{address}</span>

        <div ref={toolbarRef} className="ml-auto flex items-center gap-1">
          <InputMenu
            mode={inputSettings.mode}
            rateHz={inputSettings.rateHz}
            disabled={!connected}
            onModeChange={(mode) => setInputSettings((current) => ({ ...current, mode }))}
            onRateChange={(rateHz) => setInputSettings((current) => ({ ...current, rateHz }))}
          />
          <ShortcutsMenu sender={inputSender} disabled={!connected} />
          <StreamSettingsPicker
            info={streamSettings.info}
            error={streamSettings.error}
            disabled={!connected}
            onChange={streamSettings.update}
          />
          <StatsMenu
            basic={statsVisibility.basic}
            advanced={statsVisibility.advanced}
            onBasicChange={(basic) => setStatsVisibility((current) => ({ ...current, basic }))}
            onAdvancedChange={(advanced) => setStatsVisibility((current) => ({ ...current, advanced }))}
          />
          <ToolbarButton label={isFullscreen ? "Exit fullscreen" : "Fullscreen"} onClick={toggleFullscreen}>
            {isFullscreen ? <Minimize /> : <Maximize />}
          </ToolbarButton>
          {/* Icon-only on phones so the toolbar fits. */}
          <Button variant="outline" className="max-sm:size-8 max-sm:px-0" onClick={disconnect}>
            <Unplug aria-hidden />
            <span className="max-sm:sr-only">Disconnect</span>
          </Button>
        </div>
      </header>

      {/* Focusable so it receives the keys sent to the remote computer. */}
      <div
        ref={stageRef}
        tabIndex={0}
        role="application"
        aria-label={`Remote desktop of ${address}`}
        aria-description={`Keys go to the remote computer. Press ${LEAVE_SHORTCUT} to return to the toolbar.`}
        className="relative min-h-0 flex-1 bg-black outline-none focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:ring-inset"
      >
        <video
          ref={videoRef}
          // Touches are forwarded, so the browser mustn't pan or zoom on them.
          className="size-full touch-none object-contain"
          aria-hidden
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
        {(statsVisibility.basic || statsVisibility.advanced) && connected && connection && (
          <StatsOverlay
            connection={connection}
            source={streamSettings.info?.source}
            basic={statsVisibility.basic}
            advanced={statsVisibility.advanced}
          />
        )}
      </div>
    </div>
  )
}

function ToolbarButton({
  label,
  onClick,
  children,
}: {
  label: string
  onClick: () => void
  children: ReactNode
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={<Button variant="ghost" size="icon" aria-label={label} onClick={onClick} />}
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

/** Which stats overlay sections are shown, remembered across visits. Defaults to Basic only. */
function useStatsVisibility() {
  const [visibility, setVisibility] = useState<StatsVisibility>(() => {
    try {
      const saved = JSON.parse(readStorage(STATS_VISIBILITY_KEY) ?? "null")
      if (typeof saved?.basic === "boolean" && typeof saved?.advanced === "boolean") return saved
    } catch {
      // A malformed saved value falls back to the defaults.
    }
    return { basic: true, advanced: false }
  })

  useEffect(() => {
    writeStorage(STATS_VISIBILITY_KEY, JSON.stringify(visibility))
  }, [visibility])

  return [visibility, setVisibility] as const
}
