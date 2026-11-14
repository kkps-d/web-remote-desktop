import { useEffect, useMemo, useState, type RefObject } from "react"

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
}

/**
 * Forwards mouse input on the video and keyboard input on the stage to the connection's input
 * channel. Returns the sender, for toolbar shortcuts.
 */
export function useRemoteInput({ connection, stageRef, videoRef, onLeaveKeyboard, mode, rateHz }: RemoteInputOptions) {
  const sender = useMemo(() => connection && createInputSender(connection.input), [connection])

  useEffect(() => {
    sender?.setRate(rateHz)
  }, [sender, rateHz])

  useEffect(() => {
    const stage = stageRef.current
    if (!sender || !stage) return
    const detach = attachKeyboardInput(stage, sender, onLeaveKeyboard)
    return () => {
      detach()
      // Sent only if the channel is still open; the agent also releases input when it closes.
      sender.releaseAll()
      sender.cancelPending()
    }
  }, [sender, stageRef, onLeaveKeyboard])

  useEffect(() => {
    const video = videoRef.current
    if (!sender || !connection || !video) return

    // The stream shows its own cursor only in relative mode, where the client's is hidden.
    const sendCursor = () => sender.setStreamCursor(mode === "relative")
    sendCursor()
    connection.input.addEventListener("open", sendCursor)
    const detach = attachPointerInput(video, sender, mode)
    return () => {
      connection.input.removeEventListener("open", sendCursor)
      detach()
      sender.releaseAll()
    }
  }, [sender, connection, videoRef, mode])

  return sender
}

/** Pointer mode and send rate, remembered across visits. Defaults to absolute at 125 Hz. */
export function useInputSettings() {
  const [settings, setSettings] = useState(readInputSettings)

  useEffect(() => {
    saveInputSettings(settings)
  }, [settings])

  return [settings, setSettings] as const
}
