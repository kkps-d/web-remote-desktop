// Stream, input and audio settings remembered across visits.

import { scaleToHeight, type StreamInfo, type StreamSettings } from "./agent-connection.ts"
import type { MouseMode, TouchMode } from "./pointer-input.ts"
import { readStorage, writeStorage } from "./storage.ts"

const STREAM_SETTINGS_KEY = "streamSettings"
const INPUT_SETTINGS_KEY = "inputSettings"
const AUDIO_MUTED_KEY = "audioMuted"

export const INPUT_RATES_HZ = [125, 250, 500, 1000]

// Resolution is saved as a height (null = native) so it adapts to each agent's source size.
// Encoder is optional because settings saved before the encoder picker don't have one.
export interface SavedStreamSettings {
  height: number | null
  framerate: number
  bitrateKbps: number
  encoder?: string
}

export interface InputSettings {
  mouse: MouseMode
  touch: TouchMode
  rateHz: number
}

/** Saved stream settings, or undefined if there are none or they're malformed. */
export function parseStreamSettings(raw: string | null): SavedStreamSettings | undefined {
  try {
    const saved = JSON.parse(raw ?? "null")
    if (
      (saved?.height === null || Number.isInteger(saved?.height)) &&
      Number.isInteger(saved?.framerate) &&
      Number.isInteger(saved?.bitrateKbps) &&
      (saved?.encoder === undefined || typeof saved.encoder === "string")
    )
      return saved
  } catch {
    // A malformed saved value falls back to the agent's defaults.
  }
}

export function toSavedStreamSettings({ source, settings }: StreamInfo): SavedStreamSettings {
  return {
    height: settings.height === source.height ? null : settings.height,
    framerate: settings.framerate,
    bitrateKbps: settings.bitrateKbps,
    encoder: settings.encoder,
  }
}

/**
 * Turns saved settings into concrete ones for this agent. Heights at or above the source mean
 * native, and an encoder this agent can't run is left out so it keeps its default.
 */
export function resolveSavedSettings(saved: SavedStreamSettings, { source, encoders }: StreamInfo): Partial<StreamSettings> {
  const size = saved.height === null || saved.height >= source.height ? source : scaleToHeight(source, saved.height)
  const encoderAvailable = encoders.some((encoder) => encoder.id === saved.encoder && encoder.available)
  return {
    ...size,
    framerate: saved.framerate,
    bitrateKbps: saved.bitrateKbps,
    ...(encoderAvailable && { encoder: saved.encoder }),
  }
}

const TOUCH_MODES: TouchMode[] = ["touchscreen", "direct", "trackpad"]

/**
 * Saved input settings, or an absolute mouse and touchscreen touch at 125 Hz if there are none or
 * they're malformed. Settings saved before the touch option (mouse mode saved as `mode`) keep
 * their mouse mode and rate and get touchscreen touch.
 */
export function parseInputSettings(raw: string | null): InputSettings {
  try {
    const saved = JSON.parse(raw ?? "null")
    const mouse = saved?.mouse ?? saved?.mode
    if ((mouse === "absolute" || mouse === "relative") && INPUT_RATES_HZ.includes(saved?.rateHz))
      return { mouse, touch: TOUCH_MODES.includes(saved.touch) ? saved.touch : "touchscreen", rateHz: saved.rateHz }
  } catch {
    // A malformed saved value falls back to the defaults.
  }
  return { mouse: "absolute", touch: "touchscreen", rateHz: 125 }
}

export function readStreamSettings() {
  return parseStreamSettings(readStorage(STREAM_SETTINGS_KEY))
}

export function saveStreamSettings(info: StreamInfo) {
  writeStorage(STREAM_SETTINGS_KEY, JSON.stringify(toSavedStreamSettings(info)))
}

export function readInputSettings() {
  return parseInputSettings(readStorage(INPUT_SETTINGS_KEY))
}

export function saveInputSettings(settings: InputSettings) {
  writeStorage(INPUT_SETTINGS_KEY, JSON.stringify(settings))
}

/** Whether the user muted the stream's audio. Unmuted unless "true" is saved. */
export function parseAudioMuted(raw: string | null): boolean {
  return raw === "true"
}

export function readAudioMuted() {
  return parseAudioMuted(readStorage(AUDIO_MUTED_KEY))
}

export function saveAudioMuted(muted: boolean) {
  writeStorage(AUDIO_MUTED_KEY, String(muted))
}
