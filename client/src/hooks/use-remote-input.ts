import { useEffect, useMemo, useState, type RefObject } from "react"

import type { AgentConnection } from "@/lib/agent-connection"
import { createInputSender } from "@/lib/input-sender"
import { attachKeyboardInput } from "@/lib/keyboard-input"
import { attachPointerInput, type PointerMode } from "@/lib/pointer-input"

export const INPUT_RATES_HZ = [125, 250, 500, 1000]

const INPUT_SETTINGS_KEY = "inputSettings"

export interface InputSettings {
  mode: PointerMode
  rateHz: number
}

interface RemoteInputOptions extends InputSettings {
  connection?: AgentConnection
  stageRef: RefObject<HTMLElement | null>
  videoRef: RefObject<HTMLVideoElement | null>
}

/**
 * Forwards mouse input on the video and keyboard input on the stage to the connection's input
 * channel. Returns the sender, for toolbar shortcuts.
 */
export function useRemoteInput({ connection, stageRef, videoRef, mode, rateHz }: RemoteInputOptions) {
  const sender = useMemo(() => connection && createInputSender(connection.input), [connection])

  useEffect(() => {
    sender?.setRate(rateHz)
  }, [sender, rateHz])

  useEffect(() => {
    const stage = stageRef.current
    if (!sender || !stage) return
    const detach = attachKeyboardInput(stage, sender)
    return () => {
      detach()
      // Sent only if the channel is still open; the agent also releases input when it closes.
      sender.releaseAll()
      sender.cancelPending()
    }
  }, [sender, stageRef])

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
  const [settings, setSettings] = useState<InputSettings>(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(INPUT_SETTINGS_KEY) ?? "null")
      if ((saved?.mode === "absolute" || saved?.mode === "relative") && INPUT_RATES_HZ.includes(saved?.rateHz))
        return saved
    } catch {
      // A malformed saved value falls back to the defaults.
    }
    return { mode: "absolute", rateHz: 125 }
  })

  useEffect(() => {
    localStorage.setItem(INPUT_SETTINGS_KEY, JSON.stringify(settings))
  }, [settings])

  return [settings, setSettings] as const
}
