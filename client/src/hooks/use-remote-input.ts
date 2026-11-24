import { useEffect, useMemo, useRef, useState, type RefObject } from "react"

import type { AgentConnection } from "@/lib/agent-connection"
import { createInputSender } from "@/lib/input-sender"
import { attachKeyboardInput } from "@/lib/keyboard-input"
import { attachPointerInput } from "@/lib/pointer-input"
import { readInputSettings, saveInputSettings, type InputSettings } from "@/lib/saved-settings"

interface RemoteInputOptions extends InputSettings {
  connection?: AgentConnection
  stageRef: RefObject<HTMLElement | null>
  videoRef: RefObject<HTMLVideoElement | null>
  /** Moves focus out of the stage when the user presses the leave shortcut; keep it stable. */
  onLeaveKeyboard: () => void
  sendEscapeOnPointerRelease?: boolean
  onPointerLockChange?: (locked: boolean) => void
}

/**
 * Forwards mouse input on the video and keyboard input on the stage to the connection's input
 * channel. Returns the sender, for toolbar shortcuts.
 */
export function useRemoteInput({
  connection,
  stageRef,
  videoRef,
  onLeaveKeyboard,
  sendEscapeOnPointerRelease,
  onPointerLockChange,
  mouse,
  touch,
  rateHz,
}: RemoteInputOptions) {
  const pointerLocked = useRef(false)
  const sender = useMemo(() => connection && createInputSender(connection.input), [connection])

  useEffect(() => {
    sender?.setRate(rateHz)
  }, [sender, rateHz])

  useEffect(() => {
    const stage = stageRef.current
    if (!sender || !stage) return
    const detach = attachKeyboardInput(
      stage,
      sender,
      onLeaveKeyboard,
      sendEscapeOnPointerRelease ? () => pointerLocked.current : undefined,
    )
    return () => {
      detach()
      // Sent only if the channel is still open; the agent also releases input when it closes.
      sender.releaseAll()
      sender.cancelPending()
    }
  }, [sender, stageRef, onLeaveKeyboard, sendEscapeOnPointerRelease])

  useEffect(() => {
    const video = videoRef.current
    if (!sender || !connection || !video) return

    // The stream shows its own cursor when the client's isn't on the remote one: with a relative
    // mouse (Pointer Lock hides the client's) or trackpad touch.
    const sendCursor = () => sender.setStreamCursor(mouse === "relative" || touch === "trackpad")
    sendCursor()
    connection.input.addEventListener("open", sendCursor)
    const detach = attachPointerInput(video, sender, mouse, touch, {
      sendEscapeOnRelease: sendEscapeOnPointerRelease,
      onChange(locked) {
        pointerLocked.current = locked
        onPointerLockChange?.(locked)
      },
    })
    return () => {
      connection.input.removeEventListener("open", sendCursor)
      detach()
      if (pointerLocked.current) {
        pointerLocked.current = false
        onPointerLockChange?.(false)
      }
      sender.releaseAll()
    }
  }, [sender, connection, videoRef, mouse, touch, sendEscapeOnPointerRelease, onPointerLockChange])

  return sender
}

/** Mouse mode, touch mode and send rate, remembered across visits. Defaults to absolute, touchscreen, 125 Hz. */
export function useInputSettings() {
  const [settings, setSettings] = useState(readInputSettings)

  useEffect(() => {
    saveInputSettings(settings)
  }, [settings])

  return [settings, setSettings] as const
}
