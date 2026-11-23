import { useEffect, useEffectEvent, useRef, useState, type RefObject } from "react"

import type { AgentConnection } from "@/lib/agent-connection"
import { FRAME_PACING_WINDOW_MS, FramePacingHistory, type FrameInterval } from "@/lib/frame-pacing"
import { framePacing } from "@/lib/stat-definitions"

const REDRAW_INTERVAL_MS = 1000 / 30
const READOUT_INTERVAL_MS = 250

interface FramePacingGraphProps {
  connection: AgentConnection
  videoRef: RefObject<HTMLVideoElement | null>
  targetFps?: number
}

export function FramePacingGraph({ connection, videoRef, targetFps }: FramePacingGraphProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const scaleLabelRef = useRef<HTMLSpanElement>(null)
  const [latest, setLatest] = useState<number>()
  const [peak, setPeak] = useState<number>()
  const targetMs = targetFps !== undefined && targetFps > 0 ? 1000 / targetFps : undefined
  const supported =
    typeof HTMLVideoElement.prototype.requestVideoFrameCallback === "function" &&
    typeof HTMLVideoElement.prototype.cancelVideoFrameCallback === "function"

  const redraw = useEffectEvent((samples: readonly FrameInterval[], now: number, peak: number) => {
    const maxMs = Math.max(peak, targetMs === undefined ? 33.3 : 2 * targetMs)
    const canvas = canvasRef.current
    if (canvas) drawGraph(canvas, samples, now, maxMs, targetMs)

    const scaleLabel = scaleLabelRef.current
    const scaleText = `0–${formatMs(maxMs)}`
    if (scaleLabel && scaleLabel.textContent !== scaleText) scaleLabel.textContent = scaleText
  })

  useEffect(() => {
    const video = videoRef.current
    if (!video || !supported) return

    let history = new FramePacingHistory()
    let callbackId: number | undefined
    let animationId: number | undefined
    let readoutTimer: ReturnType<typeof setInterval> | undefined
    let nextRedrawAt = 0
    let latestMs: number | undefined
    let peakMs = 0
    const onFrame: VideoFrameRequestCallback = (_now, metadata) => {
      if (document.hidden) return
      history.add(metadata)
      callbackId = video.requestVideoFrameCallback(onFrame)
    }
    const draw = () => {
      const now = performance.now()
      const samples = history.recent(now)
      latestMs = samples.at(-1)?.durationMs
      peakMs = samples.reduce((max, sample) => Math.max(max, sample.durationMs), 0)
      redraw(samples, now, peakMs)
    }
    const updateReadouts = () => {
      setLatest(latestMs)
      setPeak(peakMs || undefined)
    }
    const animate = (now: number) => {
      if (document.hidden) return
      // Allow for sub-millisecond timestamp rounding and skip missed redraws after a delay.
      const elapsed = now + 0.5 - nextRedrawAt
      if (elapsed >= 0) {
        nextRedrawAt += (Math.floor(elapsed / REDRAW_INTERVAL_MS) + 1) * REDRAW_INTERVAL_MS
        draw()
      }
      animationId = requestAnimationFrame(animate)
    }
    const stop = () => {
      if (callbackId !== undefined) video.cancelVideoFrameCallback(callbackId)
      callbackId = undefined
      if (animationId !== undefined) cancelAnimationFrame(animationId)
      animationId = undefined
      clearInterval(readoutTimer)
    }
    const onVisibilityChange = () => {
      stop()
      history = new FramePacingHistory()
      draw()
      updateReadouts()
      if (document.hidden) return
      callbackId = video.requestVideoFrameCallback(onFrame)
      nextRedrawAt = performance.now() + REDRAW_INTERVAL_MS
      animationId = requestAnimationFrame(animate)
      readoutTimer = setInterval(updateReadouts, READOUT_INTERVAL_MS)
    }

    onVisibilityChange()
    document.addEventListener("visibilitychange", onVisibilityChange)
    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange)
      stop()
    }
  }, [connection, supported, videoRef])

  return (
    // Keep a compact preferred width while letting the grid stretch the graph to match wider stats.
    <section
      aria-label={framePacing.name}
      className="min-w-0 self-start [contain-intrinsic-inline-size:14rem] [contain:inline-size]"
    >
      <div className="flex flex-wrap items-baseline justify-between gap-x-2">
        <h2 className="font-medium">{framePacing.name}</h2>
        {supported && targetMs !== undefined && (
          <span className="inline-flex items-center gap-1 text-white/80">
            <span aria-hidden className="w-3 border-t border-dashed" />
            Target {formatMs(targetMs)}
          </span>
        )}
      </div>
      {supported ? (
        <>
          <canvas
            ref={canvasRef}
            aria-hidden
            className="mt-0.5 block h-8 w-full rounded-sm bg-white/10"
          />
          <div aria-hidden className="flex justify-between gap-x-1 text-white/70">
            <span>5 s ago</span>
            <span ref={scaleLabelRef} />
            <span>Now</span>
          </div>
          <dl
            aria-label="Frame interval summary"
            className="mt-0.5 flex flex-wrap justify-between gap-x-2 tabular-nums"
          >
            <div className="flex gap-1">
              <dt className="text-white/70">Latest</dt>
              <dd>{formatMs(latest)}</dd>
            </div>
            <div className="flex gap-1">
              <dt className="text-white/70">Peak</dt>
              <dd>{formatMs(peak)}</dd>
            </div>
          </dl>
        </>
      ) : (
        <p className="text-white/70">Unavailable in this browser</p>
      )}
    </section>
  )
}

function formatMs(value?: number) {
  return value === undefined ? "—" : `${value.toFixed(1)} ms`
}

function drawGraph(
  canvas: HTMLCanvasElement,
  samples: readonly FrameInterval[],
  now: number,
  maxMs: number,
  targetMs?: number,
) {
  const width = canvas.clientWidth
  const height = canvas.clientHeight
  const ctx = canvas.getContext("2d")
  if (!ctx || width < 2 || height < 2) return

  const pixelWidth = Math.round(width * window.devicePixelRatio)
  const pixelHeight = Math.round(height * window.devicePixelRatio)
  if (canvas.width !== pixelWidth) canvas.width = pixelWidth
  if (canvas.height !== pixelHeight) canvas.height = pixelHeight
  ctx.setTransform(pixelWidth / width, 0, 0, pixelHeight / height, 0, 0)
  ctx.clearRect(0, 0, width, height)

  const y = (ms: number) => height - 1 - (ms / maxMs) * (height - 2)
  if (targetMs !== undefined) {
    ctx.strokeStyle = "#ffffff99"
    ctx.setLineDash([3, 3])
    ctx.beginPath()
    ctx.moveTo(0, y(targetMs))
    ctx.lineTo(width, y(targetMs))
    ctx.stroke()
    ctx.setLineDash([])
  }

  ctx.strokeStyle = "#fff"
  ctx.fillStyle = "#fff"
  ctx.beginPath()
  for (const [index, { endedAt, durationMs, startsSegment }] of samples.entries()) {
    const x = 1 + (1 - (now - endedAt) / FRAME_PACING_WINDOW_MS) * (width - 2)
    const pointY = y(durationMs)
    if (!index || startsSegment) {
      ctx.fillRect(x - 0.75, pointY - 0.75, 1.5, 1.5)
      ctx.moveTo(x, pointY)
    } else {
      ctx.lineTo(x, pointY)
    }
  }
  ctx.stroke()
}
