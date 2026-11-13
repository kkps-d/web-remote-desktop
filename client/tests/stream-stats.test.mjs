import assert from "node:assert/strict"
import test from "node:test"
import { averageEncodeMs, readStreamStats } from "../src/lib/stream-stats.ts"

function report(overrides = {}) {
  return new Map([
    ["video", {
      id: "video", type: "inbound-rtp", kind: "video", timestamp: 1000,
      bytesReceived: 1000, framesDecoded: 10, totalDecodeTime: 0.1,
      packetsReceived: 100, packetsLost: 0, jitter: 0.004,
      jitterBufferDelay: 0.2, jitterBufferEmittedCount: 10,
      framesDropped: 2, freezeCount: 1, totalFreezesDuration: 0.5,
      transportId: "transport", codecId: "codec", ...overrides,
    }],
    ["transport", { selectedCandidatePairId: "selected" }],
    ["selected", { currentRoundTripTime: 0.012 }],
    ["unselected", { type: "candidate-pair", currentRoundTripTime: 1 }],
    ["codec", { mimeType: "video/H264" }],
  ])
}

test("first sample reports gauges, leaving interval metrics unavailable", () => {
  const stats = readStreamStats(report())
  assert.equal(stats.rttMs, 12)
  assert.equal(stats.jitterMs, 4)
  assert.equal(stats.codec, "H264")
  assert.equal(stats.bitrateKbps, undefined)
  assert.equal(stats.decodeMs, undefined)
  assert.equal(stats.lossPercent, undefined)
  assert.equal(stats.decoder, undefined)
})

test("calculates recent rates and timings rather than lifetime averages", () => {
  const first = readStreamStats(report())
  const stats = readStreamStats(report({
    timestamp: 2000, bytesReceived: 251000, framesDecoded: 30, totalDecodeTime: 0.16,
    packetsReceived: 198, packetsLost: 2, jitterBufferDelay: 0.3, jitterBufferEmittedCount: 30,
  }), first.sample)
  assert.equal(stats.bitrateKbps, 2000)
  assert.equal(stats.fps, 20)
  assert.equal(averageEncodeMs({ totalEncodeTimeMs: 180, framesEncoded: 30 }, { totalEncodeTimeMs: 100, framesEncoded: 10 }), 4)
  assert.ok(Math.abs(stats.decodeMs - 3) < 0.0001)
  assert.ok(Math.abs(stats.bufferMs - 5) < 0.0001)
  assert.equal(stats.lossPercent, 2)
  assert.equal(stats.droppedFrames, 2)
  assert.equal(stats.freezeSeconds, 0.5)
})

test("counter resets and stream replacement do not produce negative rates", () => {
  const first = readStreamStats(report())
  for (const changed of [{ bytesReceived: 0, framesDecoded: 0 }, { id: "replacement" }]) {
    const stats = readStreamStats(report({ timestamp: 2000, ...changed }), first.sample)
    assert.equal(stats.bitrateKbps, undefined)
    assert.equal(stats.decodeMs, undefined)
  }
})

test("missing fields, idle intervals, and late packet corrections remain unavailable", () => {
  const first = readStreamStats(report({ packetsLost: 2 }))
  const idle = readStreamStats(report({ timestamp: 2000, packetsLost: 1 }), first.sample)
  assert.equal(idle.decodeMs, undefined)
  assert.equal(averageEncodeMs({ totalEncodeTimeMs: 100, framesEncoded: 10 }, { totalEncodeTimeMs: 100, framesEncoded: 10 }), undefined)
  assert.equal(averageEncodeMs({ totalEncodeTimeMs: 100, framesEncoded: 10 }), undefined)
  assert.equal(idle.lossPercent, undefined)
  const missing = readStreamStats(new Map())
  assert.equal(missing.rttMs, undefined)
  assert.equal(missing.codec, undefined)
})
