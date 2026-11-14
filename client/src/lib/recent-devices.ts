// Agents this browser connected to, newest first, with a screenshot thumbnail of each.

import { parseAgentAddress } from "./agent-connection.ts"

const RECENT_DEVICES_KEY = "recentDevices"
const MAX_DEVICES = 8
const THUMBNAIL_WIDTH = 320

export interface RecentDevice {
  /** Normalized `host:port`, so "pc" and "pc:8080" are the same device. */
  address: string
  lastConnectedAt: number
  /** JPEG data URL of the remote screen. */
  thumbnail?: string
}

export function normalizeAddress(address: string) {
  return parseAgentAddress(address).host
}

/** Moves the device to the front with a new time, keeping its old thumbnail unless given a new one. */
export function upsertDevice(
  devices: RecentDevice[],
  address: string,
  now: number,
  thumbnail?: string,
): RecentDevice[] {
  const normalized = normalizeAddress(address)
  const existing = devices.find((device) => device.address === normalized)
  const updated: RecentDevice = { address: normalized, lastConnectedAt: now }
  const kept = thumbnail ?? existing?.thumbnail
  if (kept) updated.thumbnail = kept
  return [updated, ...devices.filter((device) => device !== existing)].slice(0, MAX_DEVICES)
}

export function removeDevice(devices: RecentDevice[], address: string): RecentDevice[] {
  return devices.filter((device) => device.address !== address)
}

/** Keeps the well-formed entries of a saved value; anything else counts as no devices. */
export function parseDevices(raw: string | null): RecentDevice[] {
  try {
    const saved = JSON.parse(raw ?? "[]")
    if (!Array.isArray(saved)) return []
    return saved.filter(
      (device) =>
        typeof device?.address === "string" &&
        Number.isFinite(device?.lastConnectedAt) &&
        (device?.thumbnail === undefined || typeof device.thumbnail === "string"),
    )
  } catch {
    return []
  }
}

export function readRecentDevices(): RecentDevice[] {
  return parseDevices(localStorage.getItem(RECENT_DEVICES_KEY))
}

export function rememberDevice(address: string, thumbnail?: string) {
  saveDevices(upsertDevice(readRecentDevices(), address, Date.now(), thumbnail))
}

export function forgetDevice(address: string): RecentDevice[] {
  const devices = removeDevice(readRecentDevices(), address)
  saveDevices(devices)
  return devices
}

function saveDevices(devices: RecentDevice[]) {
  try {
    localStorage.setItem(RECENT_DEVICES_KEY, JSON.stringify(devices))
  } catch {
    // Storage is full; the list is a convenience, so keep the previous one.
  }
}

/** A small JPEG of the video's current frame, or undefined before the first frame. */
export function captureThumbnail(video: HTMLVideoElement): string | undefined {
  if (video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA || video.videoWidth === 0) return
  const canvas = document.createElement("canvas")
  canvas.width = THUMBNAIL_WIDTH
  canvas.height = Math.round((THUMBNAIL_WIDTH * video.videoHeight) / video.videoWidth)
  canvas.getContext("2d")?.drawImage(video, 0, 0, canvas.width, canvas.height)
  return canvas.toDataURL("image/jpeg", 0.7)
}
