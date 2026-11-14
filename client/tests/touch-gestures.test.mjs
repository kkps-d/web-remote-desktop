import assert from "node:assert/strict"
import test, { beforeEach, afterEach, mock } from "node:test"
import { createTouchGestures } from "../src/lib/touch-gestures.ts"

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

test("absolute: a tap moves there and left-clicks", () => {
  const { calls, gestures, at } = setup("absolute")
  gestures.down(at(1, 100, 200))
  gestures.move(at(1, 104, 200))
  gestures.up(at(1, 104, 200))
  assert.deepEqual(calls, [["move", 0.1, 0.2], ["down", 0], ["up", 0]])
})

test("absolute: a drag past the slop presses where it started and follows the finger", () => {
  const { calls, gestures, at } = setup("absolute")
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

test("absolute: a long press right-clicks and the release adds nothing", () => {
  const { calls, gestures, at } = setup("absolute")
  gestures.down(at(1, 100, 200))
  mock.timers.tick(500)
  gestures.move(at(1, 150, 200))
  gestures.up(at(1, 150, 200))
  assert.deepEqual(calls, [["move", 0.1, 0.2], ["down", 2], ["up", 2]])
})

test("absolute: touches that start on the letterbox are ignored", () => {
  const { calls, gestures, at } = setup("absolute")
  gestures.down(at(1, -5, 200))
  gestures.move(at(1, 100, 200))
  gestures.up(at(1, 100, 200))
  assert.deepEqual(calls, [])
})

test("two fingers scroll with the content following the fingers, without clicking", () => {
  const { calls, gestures, at } = setup("absolute")
  gestures.down(at(1, 100, 500))
  gestures.down(at(2, 200, 500))
  gestures.move(at(1, 100, 480))
  gestures.move(at(2, 200, 480))
  gestures.up(at(1, 100, 480))
  gestures.up(at(2, 200, 480))
  mock.timers.tick(1000)
  // The midpoint reaches the slop at 490 without scrolling; past it, the scroll includes those 10 px.
  assert.deepEqual(calls, [["move", 0.1, 0.5], ["wheel", -0, 24]])
})

test("relative: one finger moves the cursor by the finger's movement", () => {
  const { calls, gestures, at } = setup("relative")
  gestures.down(at(1, 100, 100))
  gestures.move(at(1, 103, 98))
  gestures.move(at(1, 130, 90))
  gestures.up(at(1, 130, 90))
  mock.timers.tick(1000)
  assert.deepEqual(calls, [["moveBy", 3, -2], ["moveBy", 27, -8]])
})

test("relative: a tap clicks after waiting for a following touch", () => {
  const { calls, gestures, at } = setup("relative")
  gestures.down(at(1, 100, 100))
  gestures.up(at(1, 100, 100))
  mock.timers.tick(299)
  assert.deepEqual(calls, [])
  mock.timers.tick(1)
  assert.deepEqual(calls, [["down", 0], ["up", 0]])
})

test("relative: a long touch without moving doesn't click", () => {
  const { calls, gestures, at } = setup("relative")
  gestures.down(at(1, 100, 100))
  mock.timers.tick(400)
  gestures.up(at(1, 100, 100))
  mock.timers.tick(1000)
  assert.deepEqual(calls, [])
})

test("relative: tap then drag holds the left button for the drag, with no click", () => {
  const { calls, gestures, at } = setup("relative")
  gestures.down(at(1, 100, 100))
  gestures.up(at(1, 100, 100))
  mock.timers.tick(100)
  gestures.down(at(1, 100, 100))
  gestures.move(at(1, 120, 100))
  gestures.up(at(1, 120, 100))
  mock.timers.tick(1000)
  assert.deepEqual(calls, [["down", 0], ["moveBy", 20, 0], ["up", 0]])
})

test("relative: a double tap double-clicks at once", () => {
  const { calls, gestures, at } = setup("relative")
  gestures.down(at(1, 100, 100))
  gestures.up(at(1, 100, 100))
  mock.timers.tick(100)
  gestures.down(at(1, 100, 100))
  gestures.up(at(1, 100, 100))
  assert.deepEqual(calls, [["down", 0], ["up", 0], ["down", 0], ["up", 0]])
  mock.timers.tick(1000)
  assert.equal(calls.length, 4)
})

test("relative: a two-finger tap right-clicks", () => {
  const { calls, gestures, at } = setup("relative")
  gestures.down(at(1, 100, 100))
  gestures.down(at(2, 200, 100))
  gestures.up(at(2, 200, 100))
  gestures.up(at(1, 100, 100))
  assert.deepEqual(calls, [["down", 2], ["up", 2]])
})

test("a second finger ends a drag", () => {
  const { calls, gestures, at } = setup("absolute")
  gestures.down(at(1, 100, 200))
  gestures.move(at(1, 150, 200))
  gestures.down(at(2, 300, 200))
  assert.deepEqual(calls.slice(-1), [["up", 0]])
})

test("cancel releases a drag and drops a waiting tap", () => {
  const relative = setup("relative")
  relative.gestures.down(relative.at(1, 100, 100))
  relative.gestures.up(relative.at(1, 100, 100))
  relative.gestures.cancel()
  mock.timers.tick(1000)
  assert.deepEqual(relative.calls, [])

  const absolute = setup("absolute")
  absolute.gestures.down(absolute.at(1, 100, 200))
  absolute.gestures.move(absolute.at(1, 150, 200))
  absolute.gestures.cancel()
  assert.deepEqual(absolute.calls.slice(-1), [["up", 0]])
})
