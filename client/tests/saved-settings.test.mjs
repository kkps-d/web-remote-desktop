import assert from "node:assert/strict"
import test from "node:test"
import {
  parseAudioMuted,
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
  limits: { resolution: { width: 2560, height: 1440 }, framerate: 120, bitrateKbps: 50000 },
  audioUnavailableReason: null,
}

test("saves the resolution as a height, or null at native resolution", () => {
  assert.deepEqual(toSavedStreamSettings(info), {
    height: 1080,
    framerate: 60,
    bitrateKbps: 8000,
    encoder: "nvenc",
  })
  const native = { ...info, settings: { ...info.settings, width: 2560, height: 1440 } }
  assert.equal(toSavedStreamSettings(native).height, null)
})

test("scales a saved height to this agent's source", () => {
  const saved = { height: 720, framerate: 30, bitrateKbps: 4000, encoder: "nvenc" }
  assert.deepEqual(resolveSavedSettings(saved, info), {
    width: 1280,
    height: 720,
    framerate: 30,
    bitrateKbps: 4000,
    encoder: "nvenc",
  })
})

test("treats null and heights at or above the source as native", () => {
  for (const height of [null, 1440, 2160]) {
    const resolved = resolveSavedSettings({ height, framerate: 30, bitrateKbps: 4000 }, info)
    assert.deepEqual([resolved.width, resolved.height], [2560, 1440], String(height))
  }
})

test("lowers saved settings to the agent's limits", () => {
  const limited = {
    ...info,
    limits: { resolution: { width: 1920, height: 1080 }, framerate: 60, bitrateKbps: 25000 },
  }
  for (const height of [null, 1440]) {
    assert.deepEqual(
      resolveSavedSettings({ height, framerate: 120, bitrateKbps: 50000 }, limited),
      {
        width: 1920,
        height: 1080,
        framerate: 60,
        bitrateKbps: 25000,
      },
    )
  }
  const largest = { ...limited, settings: { ...info.settings, width: 1920, height: 1080 } }
  assert.equal(toSavedStreamSettings(largest).height, null)
})

test("leaves out an encoder this agent can't run", () => {
  for (const encoder of ["amf", "x264", undefined]) {
    const resolved = resolveSavedSettings(
      { height: null, framerate: 30, bitrateKbps: 4000, encoder },
      info,
    )
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
  assert.deepEqual(parseInputSettings('{"mouse":"relative","touch":"trackpad","rateHz":500}'), {
    mouse: "relative",
    touch: "trackpad",
    rateHz: 500,
  })
})

test("keeps the mouse mode and rate of settings saved before the touch option", () => {
  assert.deepEqual(parseInputSettings('{"mode":"relative","rateHz":500}'), {
    mouse: "relative",
    touch: "touchscreen",
    rateHz: 500,
  })
})

test("falls back to touchscreen for an unknown touch mode", () => {
  assert.deepEqual(parseInputSettings('{"mouse":"relative","touch":"native","rateHz":500}'), {
    mouse: "relative",
    touch: "touchscreen",
    rateHz: 500,
  })
})

test("falls back to an absolute mouse, touchscreen at 125 Hz for missing or unsupported input settings", () => {
  for (const raw of [
    null,
    "not json",
    '{"mouse":"touch","rateHz":500}',
    '{"mouse":"relative","rateHz":60}',
    '{"mouse":"relative"}',
  ]) {
    assert.deepEqual(
      parseInputSettings(raw),
      { mouse: "absolute", touch: "touchscreen", rateHz: 125 },
      String(raw),
    )
  }
})

test("audio is muted only when the user saved it muted", () => {
  assert.equal(parseAudioMuted("true"), true)
  for (const raw of [null, "false", "1", "not json"])
    assert.equal(parseAudioMuted(raw), false, String(raw))
})
