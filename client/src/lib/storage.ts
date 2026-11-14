// localStorage for values the app only remembers for convenience. Browsers that block site data
// throw a SecurityError on any use, and a full quota throws on writes; both count as nothing saved.

export function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

export function writeStorage(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // The value just isn't remembered.
  }
}
