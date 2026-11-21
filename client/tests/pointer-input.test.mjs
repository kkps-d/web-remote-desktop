import assert from "node:assert/strict"
import test from "node:test"
import { toVideoFraction, toWheelUnits } from "../src/lib/pointer-input.ts"

// A 1600×900 box showing a 1000×1000 video: scaled to 900×900 with 350 px bars left and right.
const box = { left: 100, top: 50, width: 1600, height: 900 }
const video = { videoWidth: 1000, videoHeight: 1000 }
const at = (x, y) => ({ clientX: box.left + x, clientY: box.top + y })

test("maps points to fractions of the content area, excluding the letterbox", () => {
  assert.deepEqual(toVideoFraction(at(350, 0), box, video), { x: 0, y: 0 })
  assert.deepEqual(toVideoFraction(at(800, 450), box, video), { x: 0.5, y: 0.5 })
  assert.deepEqual(toVideoFraction(at(1250, 900), box, video), { x: 1, y: 1 })
  assert.equal(toVideoFraction(at(349, 450), box, video), null)
  assert.equal(toVideoFraction(at(1251, 450), box, video), null)
})

test("handles bars above and below", () => {
  const wide = { videoWidth: 3200, videoHeight: 900 } // 1600×450 with 225 px bars
  assert.deepEqual(toVideoFraction(at(400, 225), box, wide), { x: 0.25, y: 0 })
  assert.equal(toVideoFraction(at(400, 224), box, wide), null)
})

test("clamps to the content edge when asked", () => {
  assert.deepEqual(toVideoFraction(at(0, 450), box, video, { clamp: true }), { x: 0, y: 0.5 })
  assert.deepEqual(toVideoFraction({ clientX: 5000, clientY: -10 }, box, video, { clamp: true }), {
    x: 1,
    y: 0,
  })
})

test("returns null until the video size is known", () => {
  assert.equal(toVideoFraction(at(800, 450), box, { videoWidth: 0, videoHeight: 0 }), null)
})

test("converts wheel deltas to Windows wheel units, positive down and right", () => {
  assert.deepEqual(toWheelUnits({ deltaX: 0, deltaY: 100, deltaMode: 0 }), { dx: 0, dy: 120 })
  assert.deepEqual(toWheelUnits({ deltaX: 0, deltaY: -3, deltaMode: 1 }), { dx: 0, dy: -120 })
  assert.deepEqual(toWheelUnits({ deltaX: 50, deltaY: 0, deltaMode: 0 }), { dx: 60, dy: 0 })
  assert.deepEqual(toWheelUnits({ deltaX: 0, deltaY: 1, deltaMode: 2 }), { dx: 0, dy: 120 })
})
