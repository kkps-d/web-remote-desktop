import assert from "node:assert/strict"
import test from "node:test"
import {
  parseInputSettings,
  parseStreamSettings,
  resolveSavedSettings,
  toSavedStreamSettings,
} from "../src/lib/saved-settings.ts"

const info = {
  source: { width: 2560, height: 1440 },
  settings: { width: 1920, height: 1080, framerate: 60, bitrateKbps: 8000, encoder: "nvenc" },
  encoders: [
    { id: "nvenc", label: "NVIDIA NVENC", available: true },
    { id: "amf", label: "AMD AMF", available: false, unavailableReason: "Not installed" },
  ],
}

test("saves the resolution as a height, or null at native resolution", () => {
  assert.deepEqual(toSavedStreamSettings(info), { height: 1080, framerate: 60, bitrateKbps: 8000, encoder: "nvenc" })
  const native = { ...info, settings: { ...info.settings, width: 2560, height: 1440 } }
  assert.equal(toSavedStreamSettings(native).height, null)
})

test("scales a saved height to this agent's source", () => {
  const saved = { height: 720, framerate: 30, bitrateKbps: 4000, encoder: "nvenc" }
  assert.deepEqual(resolveSavedSettings(saved, info), {
    width: 1280, height: 720, framerate: 30, bitrateKbps: 4000, encoder: "nvenc",
  })
})

test("treats null and heights at or above the source as native", () => {
  for (const height of [null, 1440, 2160]) {
    const resolved = resolveSavedSettings({ height, framerate: 30, bitrateKbps: 4000 }, info)
    assert.deepEqual([resolved.width, resolved.height], [2560, 1440], String(height))
  }
})

test("leaves out an encoder this agent can't run", () => {
  for (const encoder of ["amf", "x264", undefined]) {
    const resolved = resolveSavedSettings({ height: null, framerate: 30, bitrateKbps: 4000, encoder }, info)
    assert.equal("encoder" in resolved, false, String(encoder))
  }
})

test("parses saved stream settings, including ones saved before the encoder picker", () => {
  const saved = { height: 1080, framerate: 60, bitrateKbps: 8000, encoder: "nvenc" }
  assert.deepEqual(parseStreamSettings(JSON.stringify(saved)), saved)
  const legacy = { height: null, framerate: 30, bitrateKbps: 4000 }
  assert.deepEqual(parseStreamSettings(JSON.stringify(legacy)), legacy)
})

test("ignores missing or malformed stream settings", () => {
  for (const raw of [
    null,
    "not json",
    "[]",
    '{"height":"1080","framerate":60,"bitrateKbps":8000}',
    '{"height":1080,"framerate":59.94,"bitrateKbps":8000}',
    '{"height":1080,"framerate":60}',
    '{"height":1080,"framerate":60,"bitrateKbps":8000,"encoder":5}',
  ]) {
    assert.equal(parseStreamSettings(raw), undefined, String(raw))
  }
})

test("parses saved input settings", () => {
  assert.deepEqual(parseInputSettings('{"mode":"relative","rateHz":500}'), { mode: "relative", rateHz: 500 })
})

test("falls back to absolute mode at 125 Hz for missing or unsupported input settings", () => {
  for (const raw of [null, "not json", '{"mode":"touch","rateHz":500}', '{"mode":"relative","rateHz":60}', '{"mode":"relative"}']) {
    assert.deepEqual(parseInputSettings(raw), { mode: "absolute", rateHz: 125 }, String(raw))
  }
})
