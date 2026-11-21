import assert from "node:assert/strict"
import test from "node:test"
import { parseDevices, removeDevice, upsertDevice } from "../src/lib/recent-devices.ts"

test("adds devices newest first with the default port", () => {
  let devices = upsertDevice([], "192.168.1.20", 1)
  devices = upsertDevice(devices, "pc.local:9000", 2)
  assert.deepEqual(devices, [
    { address: "pc.local:9000", lastConnectedAt: 2 },
    { address: "192.168.1.20:8080", lastConnectedAt: 1 },
  ])
})

test("treats addresses with and without the default port as one device", () => {
  let devices = upsertDevice([], "192.168.1.20", 1, "old")
  devices = upsertDevice(devices, "pc", 2)
  devices = upsertDevice(devices, " 192.168.1.20:8080 ", 3)
  assert.deepEqual(devices, [
    { address: "192.168.1.20:8080", lastConnectedAt: 3, thumbnail: "old" },
    { address: "pc:8080", lastConnectedAt: 2 },
  ])
})

test("replaces the thumbnail only when given a new one", () => {
  const devices = upsertDevice([], "pc", 1, "old")
  assert.equal(upsertDevice(devices, "pc", 2, "new")[0].thumbnail, "new")
  assert.equal(upsertDevice(devices, "pc", 2)[0].thumbnail, "old")
})

test("keeps at most 8 devices, dropping the oldest", () => {
  let devices = []
  for (let i = 1; i <= 9; i++) devices = upsertDevice(devices, `10.0.0.${i}`, i)
  assert.equal(devices.length, 8)
  assert.equal(devices[0].address, "10.0.0.9:8080")
  assert.equal(devices.at(-1).address, "10.0.0.2:8080")
})

test("removes a device by address", () => {
  const devices = upsertDevice(upsertDevice([], "a", 1), "b", 2)
  assert.deepEqual(removeDevice(devices, "a:8080"), [{ address: "b:8080", lastConnectedAt: 2 }])
})

test("parses saved devices, dropping malformed ones", () => {
  const good = { address: "pc:8080", lastConnectedAt: 1, thumbnail: "data:" }
  assert.deepEqual(parseDevices(JSON.stringify([good, { address: 1 }, null, { address: "x" }])), [
    good,
  ])
  assert.deepEqual(parseDevices(null), [])
  assert.deepEqual(parseDevices("{}"), [])
  assert.deepEqual(parseDevices("not json"), [])
})
