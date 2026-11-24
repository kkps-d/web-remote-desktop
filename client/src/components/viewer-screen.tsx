import { useCallback, useEffect, useRef, type ReactNode } from "react"
import {
  LoaderCircle,
  Maximize,
  Minimize,
  MousePointer2,
  Unplug,
  Volume2,
  VolumeX,
} from "lucide-react"

import { InputMenu } from "@/components/input-menu"
import { ShortcutsMenu } from "@/components/shortcuts-menu"
import { StatsOverlay } from "@/components/stats-overlay"
import { StatsMenu } from "@/components/stats-menu"
import { StreamSettingsPicker } from "@/components/stream-settings-picker"
import { useInputSettings, useRemoteInput } from "@/hooks/use-remote-input"
import { useStreamPlayback } from "@/hooks/use-stream-playback"
import { useStreamSettings } from "@/hooks/use-stream-settings"
import { useIsFullscreen, useStatsVisibility } from "@/hooks/use-viewer-controls"
import { LEAVE_SHORTCUT } from "@/lib/keyboard-input"
import { captureThumbnail, rememberDevice } from "@/lib/recent-devices"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import type { AgentConnection } from "@/lib/agent-connection"

interface ViewerScreenProps {
  address: string
  connection?: AgentConnection
  status: "connecting" | "connected" | "reconnecting"
  onDisconnect: () => void
  useAgentDefaults: boolean
}

const statusLabels = {
  connecting: "Connecting…",
  connected: "Connected",
  reconnecting: "Reconnecting…",
}

// The first capture waits a moment so the stream has settled past its first frames.
const FIRST_THUMBNAIL_DELAY_MS = 2000
const THUMBNAIL_INTERVAL_MS = 60_000

export function ViewerScreen({
  address,
  connection,
  status,
  onDisconnect,
  useAgentDefaults,
}: ViewerScreenProps) {
  const connected = status === "connected"
  const stageRef = useRef<HTMLDivElement>(null)
  const toolbarRef = useRef<HTMLDivElement>(null)
  const videoRef = useRef<HTMLVideoElement>(null)
  const [statsVisibility, setStatsVisibility] = useStatsVisibility()
  const isFullscreen = useIsFullscreen()
  const streamSettings = useStreamSettings(connection, useAgentDefaults)
  const [inputSettings, setInputSettings] = useInputSettings()
  const inputUnavailableReason = streamSettings.info?.inputUnavailableReason
  const touchSupported = streamSettings.info?.touchSupported ?? true
  const touch =
    !touchSupported && inputSettings.touch === "touchscreen" ? "direct" : inputSettings.touch
  // The toolbar is hidden in fullscreen, so leaving the stage leaves fullscreen too.
  const focusToolbar = useCallback(() => {
    if (document.fullscreenElement) document.exitFullscreen()
    toolbarRef.current?.querySelector("button")?.focus()
  }, [])
  const inputSender = useRemoteInput({
    connection: inputUnavailableReason ? undefined : connection,
    stageRef,
    videoRef,
    onLeaveKeyboard: focusToolbar,
    ...inputSettings,
    touch,
  })

  const audio = useStreamPlayback(videoRef, address, connection)
  const audioUnavailableReason = streamSettings.info?.audioUnavailableReason

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
          {inputUnavailableReason ? (
            <ToolbarButton label={`Input unavailable: ${inputUnavailableReason}`} disabled>
              <MousePointer2 aria-hidden />
            </ToolbarButton>
          ) : (
            <InputMenu
              mouse={inputSettings.mouse}
              touch={touch}
              rateHz={inputSettings.rateHz}
              disabled={!connected}
              touchSupported={touchSupported}
              onMouseChange={(mouse) => setInputSettings((current) => ({ ...current, mouse }))}
              onTouchChange={(touch) => setInputSettings((current) => ({ ...current, touch }))}
              onRateChange={(rateHz) => setInputSettings((current) => ({ ...current, rateHz }))}
            />
          )}
          <ShortcutsMenu sender={inputSender} disabled={!connected || !!inputUnavailableReason} />
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
            onAdvancedChange={(advanced) =>
              setStatsVisibility((current) => ({ ...current, advanced }))
            }
          />
          {audioUnavailableReason ? (
            <ToolbarButton
              label={`No audio: ${audioUnavailableReason}`}
              disabled
              onClick={audio.toggleMuted}
            >
              <VolumeX />
            </ToolbarButton>
          ) : (
            <ToolbarButton
              label={audio.muted ? "Unmute audio" : "Mute audio"}
              onClick={audio.toggleMuted}
            >
              {audio.muted ? <VolumeX /> : <Volume2 />}
            </ToolbarButton>
          )}
          <ToolbarButton
            label={isFullscreen ? "Exit fullscreen" : "Fullscreen"}
            onClick={toggleFullscreen}
          >
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
        tabIndex={inputUnavailableReason ? undefined : 0}
        role="application"
        aria-label={`Remote desktop of ${address}`}
        aria-description={
          inputUnavailableReason
            ? "Remote input is unavailable."
            : `Keys go to the remote computer. Press ${LEAVE_SHORTCUT} to return to the toolbar.`
        }
        className="relative min-h-0 flex-1 bg-black outline-none focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:ring-inset"
      >
        <video
          ref={videoRef}
          // Touches are forwarded, so the browser mustn't pan or zoom on them.
          className="size-full touch-none object-contain"
          aria-hidden
          playsInline
        />
        {!connected && (
          // The dimmed backdrop keeps the text readable over the last frame of a lost stream.
          <div
            role="status"
            className="absolute inset-0 flex items-center justify-center gap-2 bg-black/60 text-sm text-white"
          >
            <LoaderCircle className="size-4 animate-spin motion-reduce:animate-none" aria-hidden />
            {status === "reconnecting"
              ? "Connection lost. Reconnecting…"
              : `Connecting to ${address}…`}
          </div>
        )}
        {(statsVisibility.basic || statsVisibility.advanced) && connected && connection && (
          <StatsOverlay
            connection={connection}
            videoRef={videoRef}
            targetFps={streamSettings.info?.settings.framerate}
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
  disabled = false,
  onClick,
  children,
}: {
  label: string
  disabled?: boolean
  onClick?: () => void
  children: ReactNode
}) {
  // A disabled button stays focusable and hoverable so its tooltip can say why.
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant="ghost"
            size="icon"
            aria-label={label}
            disabled={disabled}
            focusableWhenDisabled
            className="data-disabled:opacity-50"
            onClick={onClick}
          />
        }
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}
