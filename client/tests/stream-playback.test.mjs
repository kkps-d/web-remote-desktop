import assert from "node:assert/strict"
import test from "node:test"
import { isLocalAgent, playWithMutedFallback } from "../src/lib/stream-playback.ts"

// A video whose play() fails with the given error names, in order, then succeeds.
function fakeVideo(muted, ...failures) {
  const video = {
    muted,
    plays: [],
    play() {
      video.plays.push(video.muted)
      const name = failures.shift()
      return name ? Promise.reject(new DOMException("play failed", name)) : Promise.resolve()
    },
  }
  return video
}

test("plays unmuted when the browser allows it", async () => {
  const video = fakeVideo(false)
  assert.equal(await playWithMutedFallback(video), false)
  assert.deepEqual(video.plays, [false])
})

test("retries muted when the browser blocks unmuted playback", async () => {
  const video = fakeVideo(false, "NotAllowedError")
  assert.equal(await playWithMutedFallback(video), true)
  assert.equal(video.muted, true)
  assert.deepEqual(video.plays, [false, true])
})

test("leaves other play failures alone", async () => {
  const video = fakeVideo(false, "AbortError")
  assert.equal(await playWithMutedFallback(video), false)
  assert.equal(video.muted, false)
  assert.deepEqual(video.plays, [false])
})

test("doesn't report a block for an already muted video", async () => {
  const video = fakeVideo(true, "NotAllowedError")
  assert.equal(await playWithMutedFallback(video), false)
  assert.deepEqual(video.plays, [true])
})

test("recognizes agents on this computer", () => {
  for (const address of [
    "localhost",
    "localhost:9000",
    "127.0.0.1",
    "http://127.0.0.2:9000",
    "[::1]",
  ]) {
    assert.equal(isLocalAgent(address), true, address)
  }
  for (const address of ["192.168.1.20", "pc.local:9000", "localhost.example.com"]) {
    assert.equal(isLocalAgent(address), false, address)
  }
})
