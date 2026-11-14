// Turns touches on the stream into mouse input: direct touch in absolute mode, a trackpad in
// relative mode.

import type { InputSender } from "@/lib/input-sender"
import type { PointerMode } from "@/lib/pointer-input"

/** A touch point in client (CSS) pixels. */
export interface TouchPoint {
  id: number
  clientX: number
  clientY: number
}

export interface TouchGestures {
  down(point: TouchPoint): void
  move(point: TouchPoint): void
  up(point: TouchPoint): void
  /** Ends every touch without clicking, e.g. when the browser cancels them. */
  cancel(): void
}

/** How far a finger may wander before a touch counts as a drag rather than a tap or hold. */
export const TOUCH_SLOP_PX = 10
/** Absolute mode: holding this long without moving right-clicks. */
export const LONG_PRESS_MS = 500
/** Relative mode: longest touch that still counts as a tap. */
export const TAP_MAX_MS = 300
/** Relative mode: how long a tap's click waits for a following touch (tap then drag, or a double tap). */
export const TAP_WAIT_MS = 300

// Scrolling moves the remote content about as far as the fingers, as with the browser's own
// wheel deltas (100 px per notch of 120 wheel units).
const WHEEL_UNITS_PER_PIXEL = 120 / 100

type Sender = Pick<InputSender, "move" | "moveBy" | "wheel" | "button">

interface Options {
  /** Maps a point to video fractions (see `toVideoFraction`); null over the letterbox unless clamped. */
  toFraction: (point: TouchPoint, clamp: boolean) => { x: number; y: number } | null
  now?: () => number
}

/**
 * Absolute: tap clicks where touched, a drag past the slop drags with the left button, a long
 * press right-clicks, two fingers scroll.
 * Relative: one finger moves the cursor, a tap left-clicks after a short wait, tap then drag
 * drags with the left button, a two-finger tap right-clicks, two fingers scroll.
 */
export function createTouchGestures(
  sender: Sender,
  mode: PointerMode,
  { toFraction, now = () => performance.now() }: Options,
): TouchGestures {
  const touches = new Map<number, { x: number; y: number }>()

  // The current gesture, from the first finger down until the last finger up.
  let startedAt = 0
  let start = { x: 0, y: 0 }
  let last = { x: 0, y: 0 }
  let moved = false
  let dragging = false
  let twoFinger = false
  let longPressed = false
  // Relative: this touch began while a tap's click was waiting.
  let afterTap = false

  let longPressTimer: ReturnType<typeof setTimeout> | undefined
  let tapTimer: ReturnType<typeof setTimeout> | undefined

  function click(button: 0 | 2) {
    sender.button(button, true)
    sender.button(button, false)
  }

  function midpoint() {
    let x = 0
    let y = 0
    for (const touch of touches.values()) {
      x += touch.x
      y += touch.y
    }
    return { x: x / touches.size, y: y / touches.size }
  }

  function endDrag() {
    if (dragging) sender.button(0, false)
    dragging = false
  }

  function startTwoFinger() {
    clearTimeout(longPressTimer)
    endDrag()
    twoFinger = true
    moved = false
    start = last = midpoint()
  }

  function down(point: TouchPoint) {
    const position = { x: point.clientX, y: point.clientY }

    if (touches.size === 0) {
      // Over the letterbox there's nothing to touch in absolute mode.
      const fraction = mode === "absolute" ? toFraction(point, false) : null
      if (mode === "absolute" && !fraction) return

      touches.set(point.id, position)
      startedAt = now()
      start = last = position
      moved = dragging = twoFinger = longPressed = false

      if (mode === "absolute") {
        sender.move(fraction!.x, fraction!.y)
        longPressTimer = setTimeout(() => {
          longPressed = true
          click(2)
        }, LONG_PRESS_MS)
      } else {
        afterTap = tapTimer !== undefined
        clearTimeout(tapTimer)
        tapTimer = undefined
      }
      return
    }

    touches.set(point.id, position)
    // A second finger turns the gesture into a scroll or two-finger tap; more fingers are ignored.
    if (touches.size === 2 && !twoFinger) startTwoFinger()
  }

  function move(point: TouchPoint) {
    if (!touches.has(point.id)) return
    touches.set(point.id, { x: point.clientX, y: point.clientY })

    if (twoFinger) {
      if (touches.size < 2) return
      const current = midpoint()
      if (!moved && distance(current, start) <= TOUCH_SLOP_PX) return
      moved = true
      // Content follows the fingers: moving them up scrolls down.
      sender.wheel(-(current.x - last.x) * WHEEL_UNITS_PER_PIXEL, -(current.y - last.y) * WHEEL_UNITS_PER_PIXEL)
      last = current
      return
    }

    const current = { x: point.clientX, y: point.clientY }
    const pastSlop = moved || distance(current, start) > TOUCH_SLOP_PX

    if (mode === "absolute") {
      if (longPressed || !pastSlop) return
      if (!moved) {
        // Press where the finger first landed, then follow it.
        clearTimeout(longPressTimer)
        moved = dragging = true
        sender.button(0, true)
      }
      const fraction = toFraction(point, true)
      if (fraction) sender.move(fraction.x, fraction.y)
      return
    }

    if (pastSlop && !moved) {
      moved = true
      if (afterTap) {
        dragging = true
        sender.button(0, true)
      }
    }
    sender.moveBy(current.x - last.x, current.y - last.y)
    last = current
  }

  function up(point: TouchPoint) {
    if (!touches.delete(point.id) || touches.size > 0) return
    clearTimeout(longPressTimer)
    const quick = now() - startedAt <= TAP_MAX_MS

    if (twoFinger) {
      if (mode === "relative" && !moved && quick) click(2)
      return
    }
    if (dragging) return endDrag()

    if (mode === "absolute") {
      if (!longPressed) click(0)
      return
    }

    if (moved || !quick) return
    if (afterTap) {
      // Double tap: the waiting click and this one.
      click(0)
      click(0)
      return
    }
    tapTimer = setTimeout(() => {
      tapTimer = undefined
      click(0)
    }, TAP_WAIT_MS)
  }

  function cancel() {
    clearTimeout(longPressTimer)
    clearTimeout(tapTimer)
    tapTimer = undefined
    endDrag()
    touches.clear()
  }

  return { down, move, up, cancel }
}

function distance(a: { x: number; y: number }, b: { x: number; y: number }) {
  return Math.hypot(a.x - b.x, a.y - b.y)
}
