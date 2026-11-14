import assert from "node:assert/strict"
import test, { beforeEach, afterEach, mock } from "node:test"
import { createInputSender } from "../src/lib/input-sender.ts"

beforeEach(() => mock.timers.enable({ apis: ["setTimeout", "Date"], now: 1000 }))
afterEach(() => mock.timers.reset())

function setup(rateHz = 125) {
  const channel = {
    readyState: "open",
    messages: [],
    send(data) {
      this.messages.push(JSON.parse(data))
    },
  }
  const sender = createInputSender(channel, { rateHz, now: () => Date.now() })
  return { channel, sender }
}

test("sends the first move at once and merges later ones into the latest position", () => {
  const { channel, sender } = setup(125)
  sender.move(0.1, 0.1)
  sender.move(0.2, 0.2)
  sender.move(0.3, 0.3)
  assert.deepEqual(channel.messages, [{ type: "move", x: 0.1, y: 0.1 }])

  mock.timers.tick(7)
  assert.equal(channel.messages.length, 1)
  mock.timers.tick(1)
  assert.deepEqual(channel.messages, [
    { type: "move", x: 0.1, y: 0.1 },
    { type: "move", x: 0.3, y: 0.3 },
  ])
})

test("sends at once again after a pause", () => {
  const { channel, sender } = setup(125)
  sender.move(0.1, 0.1)
  mock.timers.tick(20)
  sender.move(0.2, 0.2)
  assert.equal(channel.messages.length, 2)
})

test("rounds the wait up so fractional intervals never exceed the rate", () => {
  const { channel, sender } = setup(300) // 3.33 ms
  sender.move(0.1, 0.1)
  sender.move(0.2, 0.2)
  mock.timers.tick(3)
  assert.equal(channel.messages.length, 1)
  mock.timers.tick(1)
  assert.equal(channel.messages.length, 2)
})

test("sums relative moves into whole pixels and carries the fractions", () => {
  const { channel, sender } = setup()
  sender.moveBy(0.6, -0.6)
  assert.deepEqual(channel.messages, [])
  sender.moveBy(0.6, -0.6)
  assert.deepEqual(channel.messages, [{ type: "moveBy", dx: 1, dy: -1 }])

  sender.moveBy(3, 4)
  sender.moveBy(2, -1)
  mock.timers.tick(8)
  assert.deepEqual(channel.messages.at(-1), { type: "moveBy", dx: 5, dy: 2 })
})

test("sums wheel deltas and carries fractional remainders", () => {
  const { channel, sender } = setup()
  sender.wheel(0, 120)
  sender.wheel(0, 50.25)
  sender.wheel(0, 50.25)
  mock.timers.tick(8)
  sender.wheel(0, 18.5)
  mock.timers.tick(8)
  assert.deepEqual(channel.messages, [
    { type: "wheel", dx: 0, dy: 120 },
    { type: "wheel", dx: 0, dy: 100 },
    { type: "wheel", dx: 0, dy: 19 },
  ])
})

test("sends buttons and keys at once, after any pending move", () => {
  const { channel, sender } = setup()
  sender.move(0.1, 0.1)
  sender.move(0.5, 0.5)
  sender.button(0, true)
  sender.key("KeyA", true)
  assert.deepEqual(channel.messages, [
    { type: "move", x: 0.1, y: 0.1 },
    { type: "move", x: 0.5, y: 0.5 },
    { type: "button", button: 0, down: true },
    { type: "key", code: "KeyA", down: true },
  ])
  mock.timers.tick(100)
  assert.equal(channel.messages.length, 4)
})

test("forwards repeated key downs and drops releases of keys that aren't held", () => {
  const { channel, sender } = setup()
  sender.key("KeyA", true)
  sender.key("KeyA", true)
  sender.key("KeyA", false)
  sender.key("KeyA", false)
  sender.button(2, false)
  assert.deepEqual(
    channel.messages.map((message) => message.down),
    [true, true, false],
  )
})

test("releaseAll forgets held input so later releases are dropped", () => {
  const { channel, sender } = setup()
  sender.key("ShiftLeft", true)
  sender.button(0, true)
  sender.releaseAll()
  sender.key("ShiftLeft", false)
  sender.button(0, false)
  assert.deepEqual(channel.messages.at(-1), { type: "releaseAll" })
  assert.equal(channel.messages.length, 3)
})

test("drops messages while the channel isn't open, without tracking them as held", () => {
  const { channel, sender } = setup()
  channel.readyState = "connecting"
  sender.move(0.5, 0.5)
  sender.key("KeyA", true)
  sender.setStreamCursor(true)
  channel.readyState = "open"
  sender.key("KeyA", false)
  mock.timers.tick(100)
  assert.deepEqual(channel.messages, [])
})

test("cancelPending drops merged input", () => {
  const { channel, sender } = setup()
  sender.move(0.1, 0.1)
  sender.move(0.2, 0.2)
  sender.cancelPending()
  mock.timers.tick(100)
  assert.equal(channel.messages.length, 1)
})

test("sends the stream cursor setting", () => {
  const { channel, sender } = setup()
  sender.setStreamCursor(false)
  assert.deepEqual(channel.messages, [{ type: "cursor", visible: false }])
})

test("applies a new rate to later waits", () => {
  const { channel, sender } = setup(125)
  sender.setRate(1000)
  sender.move(0.1, 0.1)
  sender.move(0.2, 0.2)
  mock.timers.tick(1)
  assert.equal(channel.messages.length, 2)
})
