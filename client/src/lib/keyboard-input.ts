// Forwards keys pressed while the stage (the element around the video) has focus.

import type { InputSender } from "@/lib/input-sender"

// The Keyboard Lock API isn't in TypeScript's DOM types yet.
interface KeyboardLock {
  lock(): Promise<void>
  unlock(): void
}

/** Leaves the stage without sending the keys, since every other key, Tab included, goes to the remote computer. */
export const LEAVE_SHORTCUT = "Ctrl+Alt+Home"

/**
 * Sends key presses on the focusable stage as `KeyboardEvent.code`s; auto-repeat is sent as
 * more downs. Clicking the video focuses the stage. Releases everything when focus or the tab
 * is lost, since the matching key ups would never arrive. `onLeave` runs on the leave shortcut
 * and should move focus out of the stage.
 */
export function attachKeyboardInput(
  stage: HTMLElement,
  sender: InputSender,
  onLeave: () => void,
): () => void {
  function onKey(event: KeyboardEvent) {
    // Keys without a physical code (some IME and virtual keyboard events) can't be injected.
    if (!event.code) return
    // Same as Remote Desktop's shortcut for leaving the session's keyboard.
    if (event.type === "keydown" && event.code === "Home" && event.ctrlKey && event.altKey) {
      event.preventDefault()
      // Ctrl and Alt already went down remotely, and their ups will land outside the stage.
      sender.releaseAll()
      if (document.pointerLockElement) document.exitPointerLock()
      onLeave()
      return
    }
    event.preventDefault()
    sender.key(event.code, event.type === "keydown")
  }

  function onPointerDown() {
    // The video's mousedown default is prevented, so focus doesn't move on its own.
    stage.focus({ preventScroll: true })
  }

  function onVisibilityChange() {
    if (document.hidden) sender.releaseAll()
  }

  // In fullscreen, Keyboard Lock lets shortcuts like Alt+Tab and Esc reach the remote computer.
  // It needs a secure context, so it only works over HTTP on localhost.
  const keyboard = (navigator as Navigator & { keyboard?: KeyboardLock }).keyboard
  function onFullscreenChange() {
    if (!keyboard?.lock || !window.isSecureContext) return
    if (document.fullscreenElement === stage) keyboard.lock().catch(() => {})
    else keyboard.unlock()
  }

  stage.addEventListener("keydown", onKey)
  stage.addEventListener("keyup", onKey)
  stage.addEventListener("pointerdown", onPointerDown)
  stage.addEventListener("blur", sender.releaseAll)
  document.addEventListener("visibilitychange", onVisibilityChange)
  document.addEventListener("fullscreenchange", onFullscreenChange)
  onFullscreenChange()

  return () => {
    stage.removeEventListener("keydown", onKey)
    stage.removeEventListener("keyup", onKey)
    stage.removeEventListener("pointerdown", onPointerDown)
    stage.removeEventListener("blur", sender.releaseAll)
    document.removeEventListener("visibilitychange", onVisibilityChange)
    document.removeEventListener("fullscreenchange", onFullscreenChange)
    if (keyboard?.unlock && window.isSecureContext) keyboard.unlock()
  }
}
