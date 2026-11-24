import { Maximize, Minimize } from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"

import { GameQualityPicker } from "@/components/game-quality-picker"
import { StatsMenu } from "@/components/stats-menu"
import { StatsOverlay } from "@/components/stats-overlay"
import { Button } from "@/components/ui/button"
import { useAgentViewer } from "@/hooks/use-agent-viewer"
import { useGamePlayback } from "@/hooks/use-game-playback"
import { useGameQuality } from "@/hooks/use-game-quality"
import { useRemoteInput } from "@/hooks/use-remote-input"
import { useIsFullscreen, useStatsVisibility } from "@/hooks/use-viewer-controls"
import {
  connectToAgent,
  connectToGame,
  launchAgentFromSearch,
  launchGameFromSearch,
} from "@/lib/agent-connection"
import { defaultGameQuality, gameQualityPresets, matchesGameQuality } from "@/lib/game-quality"

export function GameViewer() {
  const [game] = useState(() => launchGameFromSearch(window.location.search))
  const [address] = useState(
    () => game ?? launchAgentFromSearch(window.location.search, window.location.hostname),
  )
  const { state, connect } = useAgentViewer(game ? connectToGame : connectToAgent)
  const launch = useCallback(() => {
    if (address) void connect(address)
  }, [address, connect])

  useEffect(() => {
    if (!address) return
    const timer = window.setTimeout(launch, 0)
    const restore = (event: PageTransitionEvent) => {
      if (event.persisted) launch()
    }
    window.addEventListener("pageshow", restore)
    return () => {
      window.clearTimeout(timer)
      window.removeEventListener("pageshow", restore)
    }
  }, [address, launch])

  const viewerRef = useRef<HTMLElement>(null)
  const stageRef = useRef<HTMLDivElement>(null)
  const videoRef = useRef<HTMLVideoElement>(null)
  const [captured, setCaptured] = useState(false)
  const [stats, setStats] = useStatsVisibility()
  const isFullscreen = useIsFullscreen()
  const connected = state.status === "connected"
  const connection = "connection" in state ? state.connection : undefined
  const quality = useGameQuality(connection)
  const playback = useGamePlayback(videoRef, connection)
  const inputUnavailable = !!quality.info?.inputUnavailableReason
  const inputConnection = connected && !inputUnavailable ? connection : undefined

  const leaveKeyboard = useCallback(() => stageRef.current?.blur(), [])
  const captureChanged = useCallback(
    (locked: boolean) => {
      setCaptured(locked)
      if (!locked) leaveKeyboard()
    },
    [leaveKeyboard],
  )
  useRemoteInput({
    connection: inputConnection,
    stageRef,
    videoRef,
    onLeaveKeyboard: leaveKeyboard,
    onPointerLockChange: captureChanged,
    sendEscapeOnPointerRelease: true,
    mouse: "relative",
    touch: "trackpad",
    rateHz: 125,
  })

  const qualityIndex = quality.applied ?? defaultGameQuality
  const preset = gameQualityPresets[qualityIndex]
  const { settings } = quality.info ?? {}
  const capped = quality.applied !== undefined && settings && !matchesGameQuality(settings, preset)
  const status = !address
    ? "Open the viewer from the game coordinator."
    : state.status === "ended"
      ? "Game stream ended."
      : state.status === "failed"
        ? state.error
        : state.status === "reconnecting"
          ? "Connection lost. Reconnecting…"
          : "Connecting to the game…"

  async function toggleFullscreen() {
    try {
      if (document.fullscreenElement) await document.exitFullscreen()
      else await viewerRef.current?.requestFullscreen()
    } catch {
      // The browser may decline fullscreen.
    }
  }

  return (
    <main ref={viewerRef} className="flex h-svh flex-col bg-background text-foreground">
      {address && (
        <header className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1 border-b px-3 py-2">
          <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
            <GameQualityPicker
              container={viewerRef}
              value={quality.pendingPreset ?? qualityIndex}
              connected={connected}
              canChange={!!quality.info}
              pending={quality.pendingPreset !== undefined}
              onChange={quality.select}
            />
            {quality.error ? (
              <p role="status" className="text-xs text-muted-foreground">
                {quality.error}
              </p>
            ) : capped ? (
              <p role="status" className="text-xs text-muted-foreground">
                Limited to {settings.width} × {settings.height} at {settings.framerate} fps,{" "}
                {settings.bitrateKbps / 1000} Mbps
              </p>
            ) : null}
          </div>
          <div className="ml-auto flex items-center gap-1">
            <StatsMenu
              basic={stats.basic}
              advanced={stats.advanced}
              onBasicChange={(basic) => setStats((current) => ({ ...current, basic }))}
              onAdvancedChange={(advanced) => setStats((current) => ({ ...current, advanced }))}
            />
            {document.fullscreenEnabled && (
              <Button
                variant="ghost"
                size="icon"
                aria-label={isFullscreen ? "Exit fullscreen" : "Fullscreen"}
                onClick={() => void toggleFullscreen()}
              >
                {isFullscreen ? <Minimize aria-hidden /> : <Maximize aria-hidden />}
              </Button>
            )}
          </div>
        </header>
      )}
      <div
        ref={stageRef}
        tabIndex={inputConnection ? 0 : undefined}
        aria-label="Game stream"
        className="relative min-h-0 flex-1 bg-black outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
        onClick={playback.enableAudio}
      >
        <video
          ref={videoRef}
          className="size-full touch-none object-contain"
          aria-hidden
          playsInline
        />
        {!connected && (
          <div className="absolute inset-0 grid place-content-center justify-items-center gap-3 bg-black/60 p-4 text-center text-sm text-white">
            <p role="status">{status}</p>
            {state.status === "failed" && (
              <Button variant="secondary" size="sm" onClick={launch}>
                Retry
              </Button>
            )}
          </div>
        )}
        {connected && (
          <div className="pointer-events-none absolute inset-x-2 bottom-4 flex flex-col items-center gap-1 text-center text-sm text-white">
            {!captured && inputConnection && (
              <p className="rounded bg-black/60 px-3 py-1.5 [@media(any-pointer:coarse)_and_(not_(any-pointer:fine))]:hidden">
                Click to capture cursor
              </p>
            )}
            {inputUnavailable && (
              <p className="rounded bg-black/60 px-3 py-1.5">Game input unavailable</p>
            )}
            {playback.needsAudioGesture && (
              <p className="rounded bg-black/60 px-3 py-1.5">Click or tap to enable sound</p>
            )}
          </div>
        )}
        {connected && connection && (stats.basic || stats.advanced) && (
          <StatsOverlay
            connection={connection}
            videoRef={videoRef}
            source={quality.info?.source}
            targetFps={settings?.framerate}
            basic={stats.basic}
            advanced={stats.advanced}
          />
        )}
      </div>
    </main>
  )
}
