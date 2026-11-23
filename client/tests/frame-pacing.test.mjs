import assert from "node:assert/strict"
import test from "node:test"
import { FramePacingHistory } from "../src/lib/frame-pacing.ts"

function present(history, ...timestamps) {
  timestamps.forEach((presentationTime, index) => {
    history.add({ presentationTime, presentedFrames: index + 1 })
  })
}

test("measures steady and uneven frame intervals at their ending timestamps", () => {
  const history = new FramePacingHistory()
  present(history, 0, 16, 32, 40, 64)
  assert.deepEqual(history.recent(65), [
    { endedAt: 16, durationMs: 16, startsSegment: true },
    { endedAt: 32, durationMs: 16, startsSegment: false },
    { endedAt: 40, durationMs: 8, startsSegment: false },
    { endedAt: 64, durationMs: 24, startsSegment: false },
  ])
})

test("records a stall on the next frame even if the previous interval aged out", () => {
  const history = new FramePacingHistory()
  present(history, 0, 16)
  assert.deepEqual(history.recent(6000), [])
  history.add({ presentationTime: 6016, presentedFrames: 3 })
  assert.equal(history.recent(6016)[0].durationMs, 6000)
})

test("leaves a gap after missed callbacks and re-seeds from the latest frame", () => {
  const history = new FramePacingHistory()
  present(history, 0, 16)
  history.add({ presentationTime: 64, presentedFrames: 5 })
  history.add({ presentationTime: 80, presentedFrames: 6 })
  assert.deepEqual(history.recent(80), [
    { endedAt: 16, durationMs: 16, startsSegment: true },
    { endedAt: 80, durationMs: 16, startsSegment: true },
  ])
})

test("starts a fresh trace on timestamp rollback and skips equal timestamps", () => {
  const history = new FramePacingHistory()
  present(history, 500, 516, 516, 540)
  assert.equal(history.recent(540).at(-1).startsSegment, true)
  assert.equal(history.recent(540).length, 2)
  history.add({ presentationTime: 100, presentedFrames: 1 })
  assert.deepEqual(history.recent(100), [])
  history.add({ presentationTime: 116, presentedFrames: 2 })
  assert.deepEqual(history.recent(116), [{ endedAt: 116, durationMs: 16, startsSegment: true }])
})

test("evicts samples by count and by age, including when frames stop arriving", () => {
  const history = new FramePacingHistory()
  present(history, ...Array.from({ length: 1200 }, (_, index) => index))
  assert.equal(history.recent(1199).length, 1024)
  assert.equal(history.recent(1199)[0].endedAt, 176)
  assert.equal(history.recent(6199).length, 1)
  assert.deepEqual(history.recent(6200), [])
})
