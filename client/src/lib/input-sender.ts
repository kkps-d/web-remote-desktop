// Sends mouse and keyboard input to the agent as JSON strings over the "input" data channel.

/** DOM button numbers: left, middle, right. */
export type MouseButton = 0 | 1 | 2

export type InputMessage =
  /** Absolute position as 0–1 fractions of the video content area (letterbox excluded). */
  | { type: "move"; x: number; y: number }
  /** Relative movement in whole pixels. */
  | { type: "moveBy"; dx: number; dy: number }
  | { type: "button"; button: MouseButton; down: boolean }
  /** Windows wheel units (120 = one notch) in the DOM's directions: positive dy scrolls down, dx right. */
  | { type: "wheel"; dx: number; dy: number }
  /** `KeyboardEvent.code`; the agent maps it to a scan code. */
  | { type: "key"; code: string; down: boolean }
  | { type: "releaseAll" }
  /** Shows or hides the cursor drawn into the stream. */
  | { type: "cursor"; visible: boolean }

/** The part of `RTCDataChannel` the sender uses. */
export interface InputChannel {
  readonly readyState: RTCDataChannelState
  send(data: string): void
}

export interface InputSender {
  /** Sets the most pointer messages (moves and wheel) sent per second. */
  setRate(rateHz: number): void
  move(x: number, y: number): void
  moveBy(dx: number, dy: number): void
  wheel(dx: number, dy: number): void
  button(button: MouseButton, down: boolean): void
  key(code: string, down: boolean): void
  /** Releases every held key and button on the agent. */
  releaseAll(): void
  setStreamCursor(visible: boolean): void
  /** Drops pending pointer input and stops its timer. */
  cancelPending(): void
}

/**
 * Wraps the input channel with a leading + trailing rate limit for pointer input: the first move
 * or wheel after a pause goes out at once; later ones within the interval are merged (latest
 * position, summed deltas) and sent when it ends. Buttons and keys go out at once, after any
 * pending pointer input, so everything keeps its order. Messages are dropped while the channel
 * isn't open.
 */
export function createInputSender(
  channel: InputChannel,
  { rateHz = 125, now = () => performance.now() }: { rateHz?: number; now?: () => number } = {},
): InputSender {
  const heldKeys = new Set<string>()
  const heldButtons = new Set<MouseButton>()

  let pendingPosition: { x: number; y: number } | null = null
  // Fractional parts stay pending, so slow, fine movement and smooth scrolling still add up.
  const pendingDelta = { x: 0, y: 0 }
  const pendingWheel = { x: 0, y: 0 }
  let lastSentAt = -Infinity
  let timer: ReturnType<typeof setTimeout> | undefined

  function send(message: InputMessage): boolean {
    if (channel.readyState !== "open") return false
    channel.send(JSON.stringify(message))
    return true
  }

  function queuePointer() {
    if (timer !== undefined) return
    const wait = lastSentAt + 1000 / rateHz - now()
    if (wait <= 0) flushPointer()
    // Timers truncate fractional delays, which would fire early and exceed the rate.
    else timer = setTimeout(flushPointer, Math.ceil(wait))
  }

  function flushPointer() {
    clearTimeout(timer)
    timer = undefined
    let sent = false

    if (pendingPosition) {
      sent = send({ type: "move", ...pendingPosition })
      pendingPosition = null
    }
    const [dx, dy] = takeWhole(pendingDelta)
    if (dx || dy) sent = send({ type: "moveBy", dx, dy }) || sent
    const [wheelX, wheelY] = takeWhole(pendingWheel)
    if (wheelX || wheelY) sent = send({ type: "wheel", dx: wheelX, dy: wheelY }) || sent

    if (sent) lastSentAt = now()
  }

  return {
    setRate(rate) {
      rateHz = rate
    },
    move(x, y) {
      pendingPosition = { x, y }
      queuePointer()
    },
    moveBy(dx, dy) {
      pendingDelta.x += dx
      pendingDelta.y += dy
      queuePointer()
    },
    wheel(dx, dy) {
      pendingWheel.x += dx
      pendingWheel.y += dy
      queuePointer()
    },
    button(button, down) {
      // An up without a down (pressed before input started, or after releaseAll) is dropped.
      if (!down && !heldButtons.has(button)) return
      flushPointer()
      if (send({ type: "button", button, down }) && down) heldButtons.add(button)
      else heldButtons.delete(button)
    },
    key(code, down) {
      if (!down && !heldKeys.has(code)) return
      flushPointer()
      if (send({ type: "key", code, down }) && down) heldKeys.add(code)
      else heldKeys.delete(code)
    },
    releaseAll() {
      flushPointer()
      heldKeys.clear()
      heldButtons.clear()
      send({ type: "releaseAll" })
    },
    setStreamCursor(visible) {
      send({ type: "cursor", visible })
    },
    cancelPending() {
      clearTimeout(timer)
      timer = undefined
      pendingPosition = null
      pendingDelta.x = pendingDelta.y = pendingWheel.x = pendingWheel.y = 0
    },
  }
}

/** Removes and returns the whole parts of a pending delta, keeping the fractions. */
function takeWhole(delta: { x: number; y: number }): [number, number] {
  // `|| 0` turns -0 into 0.
  const x = Math.trunc(delta.x) || 0
  const y = Math.trunc(delta.y) || 0
  delta.x -= x
  delta.y -= y
  return [x, y]
}
