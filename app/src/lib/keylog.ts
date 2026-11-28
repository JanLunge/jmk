// A log of key presses with their timings, for seeing what a keyboard really
// does: how long after the key before it a key came, and how long it was held.

export interface KeyEntry {
  id: number
  /** What tells this key from the others, so that its release is found. */
  key: string
  label: string
  /** When it went down, in milliseconds on the clock the caller keeps. */
  at: number
  /** Milliseconds since the press before it; none for the first. */
  gap?: number
  /** Milliseconds from press to release; none while it is down, or when the release was never seen. */
  held?: number
  down: boolean
}

export interface KeyLog {
  /** Oldest first. */
  entries: KeyEntry[]
  nextId: number
  limit: number
}

export const newKeyLog = (limit = 200): KeyLog => ({ entries: [], nextId: 1, limit })

export function logPress(log: KeyLog, key: string, label: string, at: number): KeyEntry {
  // Down already: its release went unseen, so how long it was held is not known.
  for (const entry of log.entries) if (entry.down && entry.key === key) entry.down = false
  const before = log.entries[log.entries.length - 1]
  const entry: KeyEntry = { id: log.nextId++, key, label, at, gap: before ? at - before.at : undefined, down: true }
  log.entries.push(entry)
  if (log.entries.length > log.limit) log.entries.splice(0, log.entries.length - log.limit)
  return entry
}

export function logRelease(log: KeyLog, key: string, at: number): void {
  for (let index = log.entries.length - 1; index >= 0; index--) {
    const entry = log.entries[index]
    if (entry.down && entry.key === key) {
      entry.down = false
      entry.held = at - entry.at
      return
    }
  }
}

/** The releases of the keys now down will not be seen (the window lost the keyboard). */
export function logLoseHeld(log: KeyLog): void {
  for (const entry of log.entries) entry.down = false
}

export function clearLog(log: KeyLog): void {
  log.entries.splice(0)
}

/** Whole milliseconds; from ten seconds on, seconds, as nobody counts that far in milliseconds. */
export function formatMs(ms: number | undefined): string {
  if (ms === undefined) return '–'
  const whole = Math.max(0, Math.round(ms))
  return whole < 10_000 ? `${whole} ms` : `${(whole / 1000).toFixed(1)} s`
}
