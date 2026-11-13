import type { EncodingStats } from "./agent-connection"

export interface InboundVideoStats extends RTCInboundRtpStreamStats {
  decoderImplementation?: string
}

export interface StatsSample {
  video?: InboundVideoStats
}

// Counters can disappear or reset when the stream restarts. Never show a negative rate.
function delta(current?: number, previous?: number) {
  return current !== undefined && previous !== undefined && current >= previous ? current - previous : undefined
}

function average(total?: number, count?: number) {
  return total !== undefined && count !== undefined && count > 0 ? total / count : undefined
}

function milliseconds(seconds?: number) {
  return seconds === undefined ? undefined : seconds * 1000
}

export function readStreamStats(report: RTCStatsReport, previous?: StatsSample) {
  let video: InboundVideoStats | undefined
  report.forEach((entry) => {
    if (entry.type === "inbound-rtp" && entry.kind === "video") video = entry
  })
  const old = previous?.video?.id === video?.id ? previous?.video : undefined
  const elapsed = delta(video?.timestamp, old?.timestamp)
  const bytes = delta(video?.bytesReceived, old?.bytesReceived)
  const decoded = delta(video?.framesDecoded, old?.framesDecoded)
  const lost = delta(video?.packetsLost, old?.packetsLost)
  const received = delta(video?.packetsReceived, old?.packetsReceived)
  const packets = lost !== undefined && received !== undefined ? lost + received : undefined
  const transport = video?.transportId ? report.get(video.transportId) : undefined
  const pair = transport?.selectedCandidatePairId ? report.get(transport.selectedCandidatePairId) : undefined
  const codec = video?.codecId ? report.get(video.codecId) : undefined

  return {
    sample: { video },
    width: video?.frameWidth,
    height: video?.frameHeight,
    fps: video?.framesPerSecond ?? (elapsed ? average(decoded, elapsed / 1000) : undefined),
    bitrateKbps: elapsed ? average(bytes === undefined ? undefined : bytes * 8, elapsed) : undefined,
    rttMs: pair?.currentRoundTripTime === undefined ? undefined : pair.currentRoundTripTime * 1000,
    decodeMs: milliseconds(average(delta(video?.totalDecodeTime, old?.totalDecodeTime), decoded)),
    lossPercent: packets ? average(lost === undefined ? undefined : lost * 100, packets) : undefined,
    jitterMs: video?.jitter === undefined ? undefined : video.jitter * 1000,
    bufferMs: milliseconds(
      average(
        delta(video?.jitterBufferDelay, old?.jitterBufferDelay),
        delta(video?.jitterBufferEmittedCount, old?.jitterBufferEmittedCount),
      ),
    ),
    droppedFrames: video?.framesDropped,
    freezeCount: video?.freezeCount,
    freezeSeconds: video?.totalFreezesDuration,
    codec: codec?.mimeType?.replace(/^video\//, ""),
    decoder: video?.decoderImplementation,
  }
}

/** Average encode time per frame between two agent samples, in ms. */
export function averageEncodeMs(current?: EncodingStats, previous?: EncodingStats) {
  return average(
    delta(current?.totalEncodeTimeMs, previous?.totalEncodeTimeMs),
    delta(current?.framesEncoded, previous?.framesEncoded),
  )
}
