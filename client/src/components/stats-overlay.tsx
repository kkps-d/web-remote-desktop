import { useEffect, useState } from "react"

import type { AgentConnection, EncodingStats, Resolution } from "@/lib/agent-connection"
import {
  advancedStats,
  basicStats,
  type StatDefinition,
  type StatName,
} from "@/lib/stat-definitions"
import { averageEncodeMs, readStreamStats, type StatsSample } from "@/lib/stream-stats"

interface StatsOverlayProps {
  connection: AgentConnection
  source?: Resolution
  basic: boolean
  advanced: boolean
}

export function StatsOverlay({ connection, source, basic, advanced }: StatsOverlayProps) {
  const [stats, setStats] = useState<ReturnType<typeof readStreamStats>>()
  const [encodeMs, setEncodeMs] = useState<number>()

  // Browser stats don't depend on the Advanced toggle, so toggling it keeps their rate samples.
  useEffect(() => {
    let previous: StatsSample | undefined
    return pollEverySecond(async (signal) => {
      let next: ReturnType<typeof readStreamStats> | undefined
      try {
        next = readStreamStats(await connection.peerConnection.getStats(), previous)
      } catch {
        // Shown as unavailable.
      }
      if (signal.aborted) return
      previous = next?.sample
      setStats(next)
    })
  }, [connection])

  // Encode time comes from the agent over HTTP. Polling it separately keeps a slow agent
  // response from delaying the browser stats.
  useEffect(() => {
    if (!advanced) return
    let previous: EncodingStats | undefined
    const stop = pollEverySecond(async (signal) => {
      let current: EncodingStats | undefined
      try {
        current = await connection.getEncodingStats(
          AbortSignal.any([signal, AbortSignal.timeout(3000)]),
        )
      } catch {
        // Shown as unavailable.
      }
      if (signal.aborted) return
      setEncodeMs(averageEncodeMs(current, previous))
      previous = current
    })
    return () => {
      stop()
      setEncodeMs(undefined)
    }
  }, [connection, advanced])

  const values: Record<StatName, string> = {
    Stream: formatResolution(stats?.width, stats?.height),
    Bitrate:
      stats?.bitrateKbps === undefined
        ? "—"
        : stats.bitrateKbps >= 1000
          ? formatValue(stats.bitrateKbps / 1000, "Mbps")
          : formatValue(stats.bitrateKbps, "kbps", 0),
    Framerate: formatValue(stats?.fps, "fps", 0),
    "Net. RTT": formatValue(stats?.rttMs, "ms"),
    Source: formatResolution(source?.width, source?.height),
    "Encode time": formatValue(encodeMs, "ms/frame"),
    "Decode time": formatValue(stats?.decodeMs, "ms/frame"),
    "Packet loss": formatValue(stats?.lossPercent, "%"),
    Jitter: formatValue(stats?.jitterMs, "ms"),
    "Jitter buffer delay": formatValue(stats?.bufferMs, "ms"),
    "Dropped frames": formatValue(stats?.droppedFrames, "frames", 0),
    Freezes:
      stats?.freezeCount === undefined
        ? "—"
        : `${formatValue(stats.freezeCount, "freezes", 0)} / ${formatValue(stats.freezeSeconds, "s")}`,
    Codec: stats?.codec ?? "—",
    // Chromium only exposes the decoder name while the page captures a camera or mic.
    Decoder: stats?.decoder ?? (stats?.sample.video ? "Hidden by browser" : "—"),
  }

  return (
    // Ignores the pointer so clicks and scrolls reach the remote desktop beneath.
    <div className="pointer-events-none absolute top-2 left-2 max-h-[calc(100%-1rem)] max-w-[calc(100%-1rem)] overflow-hidden rounded-md bg-black/60 px-2 py-1.5 font-mono text-[0.6875rem] leading-snug text-white">
      {basic && <StatsSection title="Basic" stats={basicStats} values={values} />}
      {basic && advanced && <div className="my-1.5 border-t border-white/15" />}
      {advanced && <StatsSection title="Advanced" stats={advancedStats} values={values} />}
    </div>
  )
}

interface StatsSectionProps {
  title: string
  stats: readonly StatDefinition[]
  values: Record<StatName, string>
}

function StatsSection({ title, stats, values }: StatsSectionProps) {
  return (
    <dl aria-label={`${title} statistics`} className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-2">
      {stats.map(({ name }) => (
        <div key={name} className="contents">
          <dt className="text-white/70">{name}</dt>
          <dd className="break-words text-right tabular-nums">{values[name]}</dd>
        </div>
      ))}
    </dl>
  )
}

/**
 * Runs `task` now and again a second after each run finishes, so slow runs can't overlap or
 * update out of order. Returns a function that stops polling and aborts the current run.
 */
function pollEverySecond(task: (signal: AbortSignal) => Promise<void>) {
  const controller = new AbortController()
  let timeout: ReturnType<typeof setTimeout>
  const run = async () => {
    await task(controller.signal)
    if (!controller.signal.aborted) timeout = setTimeout(run, 1000)
  }
  void run()
  return () => {
    controller.abort()
    clearTimeout(timeout)
  }
}

function formatValue(number: number | undefined, unit: string, decimals = 1) {
  return number === undefined || !Number.isFinite(number)
    ? "—"
    : `${number.toFixed(decimals)} ${unit}`
}

function formatResolution(width?: number, height?: number) {
  return width && height ? `${width}×${height} px` : "—"
}
