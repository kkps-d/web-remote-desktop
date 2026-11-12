import { useEffect, useState } from "react"

interface VideoStats {
  width: number
  height: number
  fps: number
  bitrateKbps: number
}

/** Polls WebRTC stats once a second and shows resolution, frame rate and bitrate. */
export function StatsOverlay({ peerConnection }: { peerConnection: RTCPeerConnection }) {
  const [stats, setStats] = useState<VideoStats>()

  useEffect(() => {
    let previous: { bytes: number; timestamp: number } | undefined

    const poll = async () => {
      const report = await peerConnection.getStats()
      report.forEach((entry) => {
        if (entry.type !== "inbound-rtp" || entry.kind !== "video") return

        const bitrateKbps = previous
          ? ((entry.bytesReceived - previous.bytes) * 8) / (entry.timestamp - previous.timestamp)
          : 0
        previous = { bytes: entry.bytesReceived, timestamp: entry.timestamp }
        setStats({
          width: entry.frameWidth ?? 0,
          height: entry.frameHeight ?? 0,
          fps: entry.framesPerSecond ?? 0,
          bitrateKbps,
        })
      })
    }

    // Poll right away so the overlay appears without waiting for the first tick.
    poll()
    const interval = setInterval(poll, 1000)

    return () => clearInterval(interval)
  }, [peerConnection])

  if (!stats) return null

  return (
    <dl className="pointer-events-none absolute top-2 left-2 grid grid-cols-[auto_auto] gap-x-3 rounded-md bg-black/70 px-3 py-2 font-mono text-xs text-white">
      <dt className="text-white/60">Resolution</dt>
      <dd>
        {stats.width}×{stats.height}
      </dd>
      <dt className="text-white/60">Frame rate</dt>
      <dd>{Math.round(stats.fps)} fps</dd>
      <dt className="text-white/60">Bitrate</dt>
      <dd>{stats.bitrateKbps >= 1000 ? `${(stats.bitrateKbps / 1000).toFixed(1)} Mbps` : `${Math.round(stats.bitrateKbps)} kbps`}</dd>
    </dl>
  )
}
