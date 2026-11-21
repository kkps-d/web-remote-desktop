import assert from "node:assert/strict"
import test, { beforeEach, afterEach, mock } from "node:test"
import { createNativeTouch, createTouchGestures } from "../src/lib/touch-gestures.ts"

beforeEach(() => mock.timers.enable({ apis: ["setTimeout", "Date"], now: 1000 }))
afterEach(() => mock.timers.reset())

// Records sender calls. Points map to fractions as x / 1000, y / 1000; x < 0 is letterbox.
function setup(mode) {
  const calls = []
  const sender = {
    move: (x, y) => calls.push(["move", x, y]),
    moveBy: (dx, dy) => calls.push(["moveBy", dx, dy]),
    wheel: (dx, dy) => calls.push(["wheel", dx, dy]),
    button: (button, down) => calls.push([down ? "down" : "up", button]),
  }
  const gestures = createTouchGestures(sender, mode, {
    toFraction: ({ clientX, clientY }, clamp) =>
      clientX < 0 && !clamp ? null : { x: Math.max(0, clientX) / 1000, y: clientY / 1000 },
    now: () => Date.now(),
  })
  const at = (id, clientX, clientY) => ({ id, clientX, clientY })
  return { calls, gestures, at }
}

test("direct: a tap moves there and left-clicks", () => {
  const { calls, gestures, at } = setup("direct")
  gestures.down(at(1, 100, 200))
  gestures.move(at(1, 104, 200))
  gestures.up(at(1, 104, 200))
  assert.deepEqual(calls, [
    ["move", 0.1, 0.2],
    ["down", 0],
    ["up", 0],
  ])
})

test("direct: a drag past the slop presses where it started and follows the finger", () => {
  const { calls, gestures, at } = setup("direct")
  gestures.down(at(1, 100, 200))
  gestures.move(at(1, 150, 200))
  gestures.move(at(1, 200, 250))
  gestures.up(at(1, 200, 250))
  assert.deepEqual(calls, [
    ["move", 0.1, 0.2],
    ["down", 0],
    ["move", 0.15, 0.2],
    ["move", 0.2, 0.25],
    ["up", 0],
  ])
})

test("direct: a long press right-clicks and the release adds nothing", () => {
  const { calls, gestures, at } = setup("direct")
  gestures.down(at(1, 100, 200))
  mock.timers.tick(500)
  gestures.move(at(1, 150, 200))
  gestures.up(at(1, 150, 200))
  assert.deepEqual(calls, [
    ["move", 0.1, 0.2],
    ["down", 2],
    ["up", 2],
  ])
})

test("direct: touches that start on the letterbox are ignored", () => {
  const { calls, gestures, at } = setup("direct")
  gestures.down(at(1, -5, 200))
  gestures.move(at(1, 100, 200))
  gestures.up(at(1, 100, 200))
  assert.deepEqual(calls, [])
})

test("two fingers scroll with the content following the fingers, without clicking", () => {
  const { calls, gestures, at } = setup("direct")
  gestures.down(at(1, 100, 500))
  gestures.down(at(2, 200, 500))
  gestures.move(at(1, 100, 480))
  gestures.move(at(2, 200, 480))
  gestures.up(at(1, 100, 480))
  gestures.up(at(2, 200, 480))
  mock.timers.tick(1000)
  // The midpoint reaches the slop at 490 without scrolling; past it, the scroll includes those 10 px.
  assert.deepEqual(calls, [
    ["move", 0.1, 0.5],
    ["wheel", -0, 24],
  ])
})

test("trackpad: one finger moves the cursor by the finger's movement", () => {
  const { calls, gestures, at } = setup("trackpad")
  gestures.down(at(1, 100, 100))
  gestures.move(at(1, 103, 98))
  gestures.move(at(1, 130, 90))
  gestures.up(at(1, 130, 90))
  mock.timers.tick(1000)
  assert.deepEqual(calls, [
    ["moveBy", 3, -2],
    ["moveBy", 27, -8],
  ])
})

test("trackpad: a tap clicks after waiting for a following touch", () => {
  const { calls, gestures, at } = setup("trackpad")
  gestures.down(at(1, 100, 100))
  gestures.up(at(1, 100, 100))
  mock.timers.tick(299)
  assert.deepEqual(calls, [])
  mock.timers.tick(1)
  assert.deepEqual(calls, [
    ["down", 0],
    ["up", 0],
  ])
})

test("trackpad: a long touch without moving doesn't click", () => {
  const { calls, gestures, at } = setup("trackpad")
  gestures.down(at(1, 100, 100))
  mock.timers.tick(400)
  gestures.up(at(1, 100, 100))
  mock.timers.tick(1000)
  assert.deepEqual(calls, [])
})

test("trackpad: tap then drag holds the left button for the drag, with no click", () => {
  const { calls, gestures, at } = setup("trackpad")
  gestures.down(at(1, 100, 100))
  gestures.up(at(1, 100, 100))
  mock.timers.tick(100)
  gestures.down(at(1, 100, 100))
  gestures.move(at(1, 120, 100))
  gestures.up(at(1, 120, 100))
  mock.timers.tick(1000)
  assert.deepEqual(calls, [
    ["down", 0],
    ["moveBy", 20, 0],
    ["up", 0],
  ])
})

test("trackpad: a double tap double-clicks at once", () => {
  const { calls, gestures, at } = setup("trackpad")
  gestures.down(at(1, 100, 100))
  gestures.up(at(1, 100, 100))
  mock.timers.tick(100)
  gestures.down(at(1, 100, 100))
  gestures.up(at(1, 100, 100))
  assert.deepEqual(calls, [
    ["down", 0],
    ["up", 0],
    ["down", 0],
    ["up", 0],
  ])
  mock.timers.tick(1000)
  assert.equal(calls.length, 4)
})

test("trackpad: a two-finger tap right-clicks", () => {
  const { calls, gestures, at } = setup("trackpad")
  gestures.down(at(1, 100, 100))
  gestures.down(at(2, 200, 100))
  gestures.up(at(2, 200, 100))
  gestures.up(at(1, 100, 100))
  assert.deepEqual(calls, [
    ["down", 2],
    ["up", 2],
  ])
})

test("a second finger ends a drag", () => {
  const { calls, gestures, at } = setup("direct")
  gestures.down(at(1, 100, 200))
  gestures.move(at(1, 150, 200))
  gestures.down(at(2, 300, 200))
  assert.deepEqual(calls.slice(-1), [["up", 0]])
})

test("cancel releases a drag and drops a waiting tap", () => {
  const trackpad = setup("trackpad")
  trackpad.gestures.down(trackpad.at(1, 100, 100))
  trackpad.gestures.up(trackpad.at(1, 100, 100))
  trackpad.gestures.cancel()
  mock.timers.tick(1000)
  assert.deepEqual(trackpad.calls, [])

  const direct = setup("direct")
  direct.gestures.down(direct.at(1, 100, 200))
  direct.gestures.move(direct.at(1, 150, 200))
  direct.gestures.cancel()
  assert.deepEqual(direct.calls.slice(-1), [["up", 0]])
})

function setupNative() {
  const calls = []
  const sender = {
    touch: (contacts) => calls.push(["touch", contacts.map(({ id, x, y }) => [id, x, y])]),
    touchCancel: () => calls.push(["cancel"]),
  }
  const touch = createNativeTouch(sender, {
    toFraction: ({ clientX, clientY }, clamp) =>
      clientX < 0 && !clamp ? null : { x: Math.max(0, clientX) / 1000, y: clientY / 1000 },
  })
  const at = (id, clientX, clientY) => ({ id, clientX, clientY })
  return { calls, touch, at }
}

test("touchscreen: sends every finger on each change", () => {
  const { calls, touch, at } = setupNative()
  touch.down(at(1, 100, 200))
  touch.down(at(2, 300, 400))
  touch.move(at(1, 150, 200))
  touch.up(at(1, 150, 200))
  touch.up(at(2, 300, 400))
  assert.deepEqual(calls, [
    ["touch", [[1, 0.1, 0.2]]],
    [
      "touch",
      [
        [1, 0.1, 0.2],
        [2, 0.3, 0.4],
      ],
    ],
    [
      "touch",
      [
        [1, 0.15, 0.2],
        [2, 0.3, 0.4],
      ],
    ],
    ["touch", [[2, 0.3, 0.4]]],
    ["touch", []],
  ])
})

test("touchscreen: ignores fingers landing on the letterbox and pins drags to the edge", () => {
  const { calls, touch, at } = setupNative()
  touch.down(at(1, -50, 200))
  touch.move(at(1, 100, 200))
  touch.up(at(1, 100, 200))
  touch.down(at(2, 100, 200))
  touch.move(at(2, -50, 200))
  assert.deepEqual(calls, [
    ["touch", [[2, 0.1, 0.2]]],
    ["touch", [[2, 0, 0.2]]],
  ])
})

test("touchscreen: cancel forgets every finger", () => {
  const { calls, touch, at } = setupNative()
  touch.down(at(1, 100, 200))
  touch.cancel()
  touch.up(at(1, 100, 200))
  assert.deepEqual(calls, [["touch", [[1, 0.1, 0.2]]], ["cancel"]])
})
