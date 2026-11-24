import assert from "node:assert/strict"
import test from "node:test"

import { defaultGameQuality, fitGameQuality } from "../src/lib/game-quality.ts"

function streamInfo(width, height, framerate = 60, bitrateKbps = 50000) {
  return {
    source: { width, height },
    settings: { width, height, framerate, bitrateKbps: 8000, encoder: "agent-choice" },
    limits: { resolution: { width, height }, framerate, bitrateKbps },
  }
}

test("uses the exact game presets on a default 1080p60 launch", () => {
  const info = streamInfo(1920, 1080)
  assert.deepEqual(fitGameQuality(0, info), {
    width: 854,
    height: 480,
    framerate: 60,
    bitrateKbps: 4000,
  })
  assert.deepEqual(fitGameQuality(defaultGameQuality, info), {
    width: 1280,
    height: 720,
    framerate: 60,
    bitrateKbps: 8000,
  })
  assert.deepEqual(fitGameQuality(2, info), {
    width: 1920,
    height: 1080,
    framerate: 60,
    bitrateKbps: 12000,
  })
})

test("caps custom launch dimensions, frame rate, and bitrate independently", () => {
  const info = streamInfo(1280, 720, 30, 9000)
  assert.deepEqual(fitGameQuality(0, info), {
    width: 854,
    height: 480,
    framerate: 30,
    bitrateKbps: 4000,
  })
  assert.deepEqual(fitGameQuality(2, info), {
    width: 1280,
    height: 720,
    framerate: 30,
    bitrateKbps: 9000,
  })
  assert.equal(fitGameQuality(2, streamInfo(2560, 1440, 120)).framerate, 60)
})

test("fits the selected encoder's starting size with even dimensions", () => {
  const info = streamInfo(2560, 1440)
  Object.assign(info.settings, { width: 1600, height: 900 })
  assert.deepEqual(fitGameQuality(2, info), {
    width: 1600,
    height: 900,
    framerate: 60,
    bitrateKbps: 12000,
  })

  info.limits.resolution.width = 1279
  const { width, height } = fitGameQuality(2, info)
  assert.ok(width <= 1279)
  assert.equal(width % 2, 0)
  assert.equal(height % 2, 0)
})
