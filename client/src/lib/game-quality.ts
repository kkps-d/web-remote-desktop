import { scaleToHeight, type StreamInfo, type StreamSettings } from "./agent-connection.ts"

export const gameQualityPresets = [
  { label: "Low", width: 854, height: 480, framerate: 60, bitrateKbps: 4000 },
  { label: "Medium", width: 1280, height: 720, framerate: 60, bitrateKbps: 8000 },
  { label: "High", width: 1920, height: 1080, framerate: 60, bitrateKbps: 12000 },
]

export const defaultGameQuality = 1

type GameStreamSettings = Omit<StreamSettings, "encoder">

export function fitGameQuality(index: number, initial: StreamInfo): GameStreamSettings {
  const preset = gameQualityPresets[index]
  const { source, settings, limits } = initial
  // The first settings describe the largest size the chosen encoder can use with this source.
  const maxWidth = Math.min(preset.width, source.width, limits.resolution.width, settings.width)
  const evenWidth = Math.floor(maxWidth / 2) * 2
  const maxHeight = Math.min(
    preset.height,
    source.height,
    limits.resolution.height,
    settings.height,
  )
  let size = scaleToHeight(source, Math.floor(maxHeight / 2) * 2)
  if (size.width > evenWidth) {
    const height = Math.floor((evenWidth * source.height) / source.width / 2) * 2
    size = scaleToHeight(source, height)
  }
  return {
    ...size,
    framerate: Math.min(preset.framerate, limits.framerate),
    bitrateKbps: Math.min(preset.bitrateKbps, limits.bitrateKbps),
  }
}

export function matchesGameQuality(actual: GameStreamSettings, target: GameStreamSettings) {
  return (
    actual.width === target.width &&
    actual.height === target.height &&
    actual.framerate === target.framerate &&
    actual.bitrateKbps === target.bitrateKbps
  )
}
