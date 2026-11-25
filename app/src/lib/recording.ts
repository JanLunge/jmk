// Turns keys recorded on the computer's keyboard into macro steps.
import { ref } from 'vue'
import type { MacroStep } from './protocol'

/** True while a macro is being recorded: every key pressed belongs to the recording, not to the page. */
export const recordingKeys = ref(false)

/** One key going down or up while recording, with the time it happened in milliseconds. */
export interface Stroke {
  usage: number
  down: boolean
  at: number
}

/**
 * How a recording is played back.
 * order: the same keys in the same order, with one fixed pause between keys (or none).
 * played: with the pauses and hold times of the recording.
 */
export type Timing = 'order' | 'played'

/** The keyboard already leaves this long between two steps. */
const STEP_MS = 2
/** Pauses shorter than this are not worth a step. */
const SHORTEST_MS = 5
const LONGEST_MS = 60_000

/** Strokes that make sense as a sequence: no repeats of a held key, no release of a key that is not down, nothing left held at the end. */
function tidy(strokes: Stroke[]): Stroke[] {
  const held = new Set<number>()
  const out: Stroke[] = []
  for (const stroke of strokes) {
    if (stroke.down === held.has(stroke.usage)) continue
    if (stroke.down) held.add(stroke.usage)
    else held.delete(stroke.usage)
    out.push(stroke)
  }
  const end = out[out.length - 1]?.at ?? 0
  // Let go in the reverse order, the way a hand does: the key pressed last comes up first.
  for (const usage of [...held].reverse()) out.push({ usage, down: false, at: end })
  return out
}

export function recordingToSteps(recorded: Stroke[], timing: Timing, pauseMs = 0): MacroStep[] {
  const strokes = tidy(recorded)
  const steps: MacroStep[] = []
  if (timing === 'played') {
    strokes.forEach((stroke, n) => {
      const wait = n ? Math.round(stroke.at - strokes[n - 1].at) - STEP_MS : 0
      if (wait >= SHORTEST_MS) steps.push({ DelayMs: Math.min(LONGEST_MS, wait) })
      steps.push(stroke.down ? { Press: stroke.usage } : { Release: stroke.usage })
    })
    return steps
  }
  for (let n = 0; n < strokes.length; n++) {
    const stroke = strokes[n]
    if (!stroke.down) {
      steps.push({ Release: stroke.usage })
      continue
    }
    if (pauseMs > 0 && steps.length) steps.push({ DelayMs: pauseMs })
    const next = strokes[n + 1]
    // Down and straight up again is a tap, which takes one step instead of two.
    if (next && !next.down && next.usage === stroke.usage) {
      steps.push({ Tap: stroke.usage })
      n++
    } else {
      steps.push({ Press: stroke.usage })
    }
  }
  return steps
}
