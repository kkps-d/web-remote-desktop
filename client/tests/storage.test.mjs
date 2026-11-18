import assert from "node:assert/strict"
import test from "node:test"
import { forgetDevice, readRecentDevices, rememberDevice } from "../src/lib/recent-devices.ts"
import { readInputSettings, readStreamSettings, saveInputSettings, saveStreamSettings } from "../src/lib/saved-settings.ts"
import { readStorage, writeStorage } from "../src/lib/storage.ts"

function useStorage(t, storage) {
  Object.defineProperty(globalThis, "localStorage", { configurable: true, get: () => storage })
  t.after(() => delete globalThis.localStorage)
}

// Browsers that block site data throw a SecurityError as soon as localStorage is read.
function blockStorage(t) {
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    get() {
      throw new DOMException("Access is denied for this document.", "SecurityError")
    },
  })
  t.after(() => delete globalThis.localStorage)
}

test("reads and writes storage", (t) => {
  const values = new Map()
  useStorage(t, { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, value) })

  writeStorage("key", "value")
  assert.equal(readStorage("key"), "value")
  assert.equal(readStorage("missing"), null)
})

test("treats blocked storage as empty and ignores writes", (t) => {
  blockStorage(t)

  assert.equal(readStorage("key"), null)
  assert.doesNotThrow(() => writeStorage("key", "value"))
})

test("ignores writes when storage is full", (t) => {
  useStorage(t, {
    getItem: () => null,
    setItem() {
      throw new DOMException("The quota has been exceeded.", "QuotaExceededError")
    },
  })

  assert.doesNotThrow(() => writeStorage("key", "value"))
})

test("saved settings and recent devices work without storage", (t) => {
  blockStorage(t)

  assert.equal(readStreamSettings(), undefined)
  assert.deepEqual(readInputSettings(), { mouse: "absolute", touch: "touchscreen", rateHz: 125 })
  assert.deepEqual(readRecentDevices(), [])
  assert.doesNotThrow(() => {
    saveStreamSettings({
      source: { width: 1920, height: 1080 },
      settings: { width: 1920, height: 1080, framerate: 60, bitrateKbps: 8000, encoder: "nvenc" },
      encoders: [],
      limits: { resolution: { width: 1920, height: 1080 }, framerate: 120, bitrateKbps: 50000 },
      audioUnavailableReason: null,
    })
    saveInputSettings({ mouse: "relative", touch: "trackpad", rateHz: 250 })
    rememberDevice("pc")
  })
  assert.deepEqual(forgetDevice("pc:8080"), [])
})
