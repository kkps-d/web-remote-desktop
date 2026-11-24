import { useEffect, useState } from "react"

import { readStorage, writeStorage } from "@/lib/storage"

const STATS_VISIBILITY_KEY = "statsVisibility"

interface StatsVisibility {
  basic: boolean
  advanced: boolean
}

export function useIsFullscreen() {
  const [isFullscreen, setIsFullscreen] = useState(() => document.fullscreenElement != null)

  useEffect(() => {
    const update = () => setIsFullscreen(document.fullscreenElement != null)
    document.addEventListener("fullscreenchange", update)
    return () => document.removeEventListener("fullscreenchange", update)
  }, [])

  return isFullscreen
}

/** Which stats overlay sections are shown, remembered across visits. Defaults to Basic only. */
export function useStatsVisibility() {
  const [visibility, setVisibility] = useState<StatsVisibility>(() => {
    try {
      const saved = JSON.parse(readStorage(STATS_VISIBILITY_KEY) ?? "null")
      if (typeof saved?.basic === "boolean" && typeof saved?.advanced === "boolean") return saved
    } catch {
      // A malformed saved value falls back to the defaults.
    }
    return { basic: true, advanced: false }
  })

  useEffect(() => {
    writeStorage(STATS_VISIBILITY_KEY, JSON.stringify(visibility))
  }, [visibility])

  return [visibility, setVisibility] as const
}
