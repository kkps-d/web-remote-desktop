// Turns mouse and touch events on the stream's <video> into input messages.

import type { InputSender, MouseButton } from "@/lib/input-sender"
// Relative so the node tests, which have no "@/" alias, can load this module.
import { createNativeTouch, createTouchGestures } from "./touch-gestures.ts"

/**
 * Absolute sends positions on the video; relative sends movement under Pointer Lock and shows
 * the stream's own cursor instead of the client's.
 */
export type MouseMode = "absolute" | "relative"

/**
 * Touchscreen injects real Windows touch. Direct and trackpad emulate a mouse: the finger is the
 * pointer where it touches, or moves the cursor like a trackpad.
 */
export type TouchMode = "touchscreen" | "direct" | "trackpad"

// Windows sends 120 units per wheel notch. Chromium reports one notch as 100 pixels and
// Firefox in line mode as 3 lines.
const WHEEL_UNITS_PER_PIXEL = 120 / 100
const WHEEL_UNITS_PER_LINE = 120 / 3
const WHEEL_UNITS_PER_PAGE = 120

// `MouseEvent.buttons` bits for the DOM button numbers 0 (left), 1 (middle) and 2 (right).
const BUTTON_BITS = [1, 4, 2]

interface Box {
  left: number
  top: number
  width: number
  height: number
}

/**
 * A point's position as 0–1 fractions of the video's content area, which `object-contain`
 * letterboxes inside the element's box. Returns null over the letterbox bars unless `clamp` is
 * set (for drags that leave the video), and while the video size is unknown.
 */
export function toVideoFraction(
  point: { clientX: number; clientY: number },
  box: Box,
  video: { videoWidth: number; videoHeight: number },
  { clamp = false } = {},
): { x: number; y: number } | null {
  if (!video.videoWidth || !video.videoHeight || !box.width || !box.height) return null
  const scale = Math.min(box.width / video.videoWidth, box.height / video.videoHeight)
  const width = video.videoWidth * scale
  const height = video.videoHeight * scale
  const x = (point.clientX - box.left - (box.width - width) / 2) / width
  const y = (point.clientY - box.top - (box.height - height) / 2) / height
  if (clamp) return { x: clampFraction(x), y: clampFraction(y) }
  return x < 0 || x > 1 || y < 0 || y > 1 ? null : { x, y }
}

/** Converts a wheel event's deltas to Windows wheel units, keeping the DOM's directions (the agent flips dy). */
export function toWheelUnits({ deltaX, deltaY, deltaMode }: Pick<WheelEvent, "deltaX" | "deltaY" | "deltaMode">) {
  const units = [WHEEL_UNITS_PER_PIXEL, WHEEL_UNITS_PER_LINE, WHEEL_UNITS_PER_PAGE][deltaMode] ?? WHEEL_UNITS_PER_PIXEL
  return { dx: deltaX * units, dy: deltaY * units }
}

/** Forwards mouse and touch input on the video in the given modes. Returns a function that detaches it. */
export function attachPointerInput(
  video: HTMLVideoElement,
  sender: InputSender,
  mouse: MouseMode,
  touchMode: TouchMode,
): () => void {
  const isLocked = () => document.pointerLockElement === video
  const toFraction = (point: { clientX: number; clientY: number }, clamp: boolean) =>
    toVideoFraction(point, video.getBoundingClientRect(), video, { clamp })
  const touch =
    touchMode === "touchscreen"
      ? createNativeTouch(sender, { toFraction })
      : createTouchGestures(sender, touchMode, { toFraction })
  const touchPoint = (event: PointerEvent) => ({ id: event.pointerId, clientX: event.clientX, clientY: event.clientY })

  // Relative input only flows while the pointer is locked to the video.
  function isActive(event: PointerEvent) {
    return event.pointerType !== "touch" && (mouse === "absolute" || isLocked())
  }

  function sendPosition(event: PointerEvent) {
    // While a button is held, a drag past the content area pins to its edge.
    const position = toVideoFraction(event, video.getBoundingClientRect(), video, { clamp: event.buttons !== 0 })
    if (position) sender.move(position.x, position.y)
  }

  function onPointerMove(event: PointerEvent) {
    if (event.pointerType === "touch") return touch.move(touchPoint(event))
    if (!isActive(event)) return
    if (mouse === "relative") sender.moveBy(event.movementX, event.movementY)
    else sendPosition(event)

    // Pressing or releasing a button while another is held fires pointermove, not pointerdown/up.
    if (isMouseButton(event.button)) sender.button(event.button, (event.buttons & BUTTON_BITS[event.button]) !== 0)
  }

  function onPointerDown(event: PointerEvent) {
    if (event.pointerType === "touch") return touch.down(touchPoint(event))
    if (mouse === "relative" && !isLocked()) {
      // This click only takes the lock; it isn't sent.
      requestPointerLock(video)
      return
    }
    if (!isMouseButton(event.button)) return
    if (mouse === "absolute") {
      // Clicks on the letterbox bars aren't sent.
      const position = toVideoFraction(event, video.getBoundingClientRect(), video)
      if (!position) return
      sender.move(position.x, position.y)
      // Keeps the drag's moves and release coming when the pointer leaves the video.
      video.setPointerCapture(event.pointerId)
    }
    sender.button(event.button, true)
  }

  function onPointerUp(event: PointerEvent) {
    if (event.pointerType === "touch") return touch.up(touchPoint(event))
    if (isActive(event) && isMouseButton(event.button)) sender.button(event.button, false)
  }

  function onPointerCancel(event: PointerEvent) {
    if (event.pointerType === "touch") return touch.cancel()
    // A canceled pen or mouse gesture gets no pointerup; the sender ignores buttons that aren't held.
    for (const button of [0, 1, 2] as const) sender.button(button, false)
  }

  function onWheel(event: WheelEvent) {
    if (mouse === "relative" && !isLocked()) return
    // Stops the page from scrolling or zooming (Ctrl is forwarded as a key).
    event.preventDefault()
    const { dx, dy } = toWheelUnits(event)
    sender.wheel(dx, dy)
  }

  // Middle-click autoscroll and the context menu would get in the way of the remote desktop.
  function preventDefault(event: Event) {
    event.preventDefault()
  }

  function onPointerLockChange() {
    // Leaving the lock (Esc) can swallow releases, so nothing may stay pressed.
    if (!isLocked()) sender.releaseAll()
  }

  video.addEventListener("pointermove", onPointerMove)
  video.addEventListener("pointerdown", onPointerDown)
  video.addEventListener("pointerup", onPointerUp)
  video.addEventListener("pointercancel", onPointerCancel)
  video.addEventListener("wheel", onWheel, { passive: false })
  video.addEventListener("mousedown", preventDefault)
  video.addEventListener("contextmenu", preventDefault)
  if (mouse === "relative") document.addEventListener("pointerlockchange", onPointerLockChange)

  return () => {
    video.removeEventListener("pointermove", onPointerMove)
    video.removeEventListener("pointerdown", onPointerDown)
    video.removeEventListener("pointerup", onPointerUp)
    video.removeEventListener("pointercancel", onPointerCancel)
    video.removeEventListener("wheel", onWheel)
    video.removeEventListener("mousedown", preventDefault)
    video.removeEventListener("contextmenu", preventDefault)
    document.removeEventListener("pointerlockchange", onPointerLockChange)
    if (isLocked()) document.exitPointerLock()
    touch.cancel()
  }
}

async function requestPointerLock(video: HTMLVideoElement) {
  try {
    // Raw movement skips the client OS's pointer acceleration, so the remote cursor moves 1:1.
    await video.requestPointerLock({ unadjustedMovement: true })
  } catch {
    try {
      await video.requestPointerLock()
    } catch {
      // Denied, for example when re-locking too soon after leaving; the next click tries again.
    }
  }
}

function isMouseButton(button: number): button is MouseButton {
  return button === 0 || button === 1 || button === 2
}

function clampFraction(value: number) {
  return Math.min(1, Math.max(0, value))
}
