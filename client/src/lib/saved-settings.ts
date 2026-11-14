// Stream and input settings remembered across visits.

import { scaleToHeight, type StreamInfo, type StreamSettings } from "./agent-connection.ts"
import type { PointerMode } from "./pointer-input.ts"

const STREAM_SETTINGS_KEY = "streamSettings"
const INPUT_SETTINGS_KEY = "inputSettings"

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
  mode: PointerMode
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

/** Saved input settings, or absolute mode at 125 Hz if there are none or they're malformed. */
export function parseInputSettings(raw: string | null): InputSettings {
  try {
    const saved = JSON.parse(raw ?? "null")
    if ((saved?.mode === "absolute" || saved?.mode === "relative") && INPUT_RATES_HZ.includes(saved?.rateHz))
      return { mode: saved.mode, rateHz: saved.rateHz }
  } catch {
    // A malformed saved value falls back to the defaults.
  }
  return { mode: "absolute", rateHz: 125 }
}

export function readStreamSettings() {
  return parseStreamSettings(readStorage(STREAM_SETTINGS_KEY))
}

export function saveStreamSettings(info: StreamInfo) {
  localStorage.setItem(STREAM_SETTINGS_KEY, JSON.stringify(toSavedStreamSettings(info)))
}

export function readInputSettings() {
  return parseInputSettings(readStorage(INPUT_SETTINGS_KEY))
}

export function saveInputSettings(settings: InputSettings) {
  localStorage.setItem(INPUT_SETTINGS_KEY, JSON.stringify(settings))
}

function readStorage(key: string) {
  try {
    return localStorage.getItem(key)
  } catch {
    // Storage is blocked (a SecurityError), so there are no saved settings.
    return null
  }
}
