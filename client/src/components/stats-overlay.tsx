import { useEffect, useState } from "react"

import type { AgentConnection, EncodingStats, Resolution } from "@/lib/agent-connection"
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
        current = await connection.getEncodingStats(AbortSignal.any([signal, AbortSignal.timeout(3000)]))
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

  const basicRows = [
    ["Stream", resolution(stats?.width, stats?.height)],
    ["Bitrate", stats?.bitrateKbps === undefined ? "—" : stats.bitrateKbps >= 1000
      ? value(stats.bitrateKbps / 1000, "Mbps") : value(stats.bitrateKbps, "kbps", 0)],
    ["Framerate", value(stats?.fps, "fps", 0)],
    ["Net. RTT", value(stats?.rttMs, "ms")],
  ]
  const advancedRows = [
    ["Source", resolution(source?.width, source?.height)],
    ["Encode time", value(encodeMs, "ms/frame")],
    ["Decode time", value(stats?.decodeMs, "ms/frame")],
    ["Packet loss", value(stats?.lossPercent, "%")],
    ["Jitter", value(stats?.jitterMs, "ms")],
    ["Jitter buffer delay", value(stats?.bufferMs, "ms")],
    ["Dropped frames", value(stats?.droppedFrames, "frames", 0)],
    ["Freezes", stats?.freezeCount === undefined ? "—"
      : `${value(stats.freezeCount, "freezes", 0)} / ${value(stats.freezeSeconds, "s")}`],
    ["Codec", stats?.codec ?? "—"],
    ["Decoder", stats?.decoder ?? "—"],
  ]

  return (
    <div className="absolute top-2 left-2 max-h-[calc(100%-1rem)] max-w-[calc(100%-1rem)] overflow-y-auto overscroll-contain rounded-md bg-black/70 px-3 py-2 font-mono text-xs text-white">
      {basic && <StatsSection label="Basic" rows={basicRows} />}
      {basic && advanced && <div className="my-2 border-t border-white/15" />}
      {advanced && <StatsSection label="Advanced" rows={advancedRows} />}
    </div>
  )
}

function StatsSection({ label, rows }: { label: string; rows: string[][] }) {
  return (
    <dl aria-label={`${label} statistics`} className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5">
      {rows.map(([label, value]) => (
        <div key={label} className="contents">
          <dt className="text-white/60">{label}</dt>
          <dd className="break-words text-right tabular-nums">{value}</dd>
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

function value(number: number | undefined, unit: string, decimals = 1) {
  return number === undefined || !Number.isFinite(number) ? "—" : `${number.toFixed(decimals)} ${unit}`
}

function resolution(width?: number, height?: number) {
  return width && height ? `${width}×${height} px` : "—"
}
