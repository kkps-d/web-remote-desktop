export const FRAME_PACING_WINDOW_MS = 5000
const MAX_SAMPLES = 1024

type Frame = Pick<VideoFrameCallbackMetadata, "presentationTime" | "presentedFrames">

export interface FrameInterval {
  endedAt: number
  durationMs: number
  startsSegment: boolean
}

export class FramePacingHistory {
  private previous?: Frame
  private startsSegment = true
  private intervals: FrameInterval[] = []

  add(frame: Frame) {
    const previous = this.previous
    this.previous = frame
    if (!previous) return

    const durationMs = frame.presentationTime - previous.presentationTime
    if (durationMs < 0) this.intervals.length = 0
    // A missed callback doesn't reveal the individual intervals between presentations.
    if (durationMs <= 0 || frame.presentedFrames !== previous.presentedFrames + 1) {
      this.startsSegment = true
    } else {
      this.intervals.push({
        endedAt: frame.presentationTime,
        durationMs,
        startsSegment: this.startsSegment,
      })
      this.startsSegment = false
    }
    this.recent(frame.presentationTime)
  }

  recent(now: number): readonly FrameInterval[] {
    const cutoff = now - FRAME_PACING_WINDOW_MS
    let discard = Math.max(0, this.intervals.length - MAX_SAMPLES)
    while (discard < this.intervals.length && this.intervals[discard].endedAt < cutoff) discard++
    if (discard) this.intervals.splice(0, discard)
    return this.intervals
  }
}
