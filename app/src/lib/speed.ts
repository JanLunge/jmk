// How fast something is typed outside a test: the characters as they come,
// gathered into bursts. A burst is typing without a pause; its speed is what
// a typist means by "how fast I type", where the time from the first key of
// the day to the last one says little.

/** A pause this long ends a burst. */
export const PAUSE_MS = 2000
/** Fewer characters than this say nothing about speed. */
export const MIN_CHARS = 5

export interface Burst {
  start: number
  end: number
  chars: number
}

export interface Speed {
  /** Oldest first; the last one may still be going. */
  bursts: Burst[]
}

export const newSpeed = (): Speed => ({ bursts: [] })

/** A character was typed at `at` (milliseconds on the clock the caller keeps). */
export function speedChar(speed: Speed, at: number): void {
  const last = speed.bursts[speed.bursts.length - 1]
  if (last && at - last.end <= PAUSE_MS && at >= last.end) {
    last.end = at
    last.chars++
  } else {
    speed.bursts.push({ start: at, end: at, chars: 1 })
    // What a long session leaves behind: the oldest go, the numbers are of the recent ones.
    if (speed.bursts.length > 500) speed.bursts.splice(0, speed.bursts.length - 500)
  }
}

export function clearSpeed(speed: Speed): void {
  speed.bursts.splice(0)
}

/** Words per minute for characters typed over a time, a word being five characters. */
const wpm = (chars: number, ms: number) => (ms > 0 ? chars / 5 / (ms / 60_000) : 0)

/** A burst's speed. The first character starts the clock, so it is the ones after it that took the time. */
export const burstWpm = (burst: Burst) => wpm(burst.chars - 1, burst.end - burst.start)

const counts = (burst: Burst) => burst.chars >= MIN_CHARS && burst.end > burst.start

export interface SpeedStats {
  /** The burst typed last that is long enough to have a speed. */
  last?: number
  best?: number
  /** All bursts together: the speed while typing, pauses left out. */
  typing?: number
  /** From the first character to the last, pauses and all. */
  overall?: number
  chars: number
  /** Bursts long enough to have a speed. */
  bursts: number
  /** Time spent in those bursts. */
  typingMs: number
}

export function speedStats(speed: Speed): SpeedStats {
  const all = speed.bursts
  const good = all.filter(counts)
  const chars = all.reduce((sum, burst) => sum + burst.chars, 0)
  const typingMs = good.reduce((sum, burst) => sum + burst.end - burst.start, 0)
  const span = all.length ? all[all.length - 1].end - all[0].start : 0
  return {
    last: good.length ? burstWpm(good[good.length - 1]) : undefined,
    best: good.length ? Math.max(...good.map(burstWpm)) : undefined,
    typing: good.length ? wpm(good.reduce((sum, burst) => sum + burst.chars - 1, 0), typingMs) : undefined,
    overall: chars >= MIN_CHARS && span > 0 ? wpm(chars - 1, span) : undefined,
    chars,
    bursts: good.length,
    typingMs,
  }
}
