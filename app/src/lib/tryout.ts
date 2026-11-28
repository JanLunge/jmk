// Trying a keyboard out from the editor: the keys it sent, with timings, what
// they typed, and whether the typing test is open. One per open editor.

import { nextTick } from 'vue'
import { ReportReader, type Stroke } from './keyfeed'
import { clearLog, logLoseHeld, logPress, logRelease, newKeyLog, type KeyLog } from './keylog'
import { onHostEvent } from './session'

export interface Tryout {
  log: KeyLog
  /** What a virtual keyboard typed into the field in the header. */
  text: string
  /** The typing test is open. */
  expanded: boolean
  /** The typing test, while open, hears every key. */
  hear?: (stroke: Stroke, at: number) => void
}

export const newTryout = (): Tryout => ({ log: newKeyLog(), text: '', expanded: false })

/** A key went down or up at `at`. `typing`: what it types goes into the field's text, as a virtual keyboard has no field of the computer's to type into. */
export function takeStroke(tryout: Tryout, stroke: Stroke, at: number, typing = false): void {
  if (stroke.down) logPress(tryout.log, stroke.key, stroke.label, at)
  else logRelease(tryout.log, stroke.key, at)
  if (typing && stroke.down && !tryout.expanded) {
    if (stroke.kind === 'backspace') tryout.text = stroke.word ? tryout.text.replace(/\S*\s*$/, '') : [...tryout.text].slice(0, -1).join('')
    else if (stroke.text) tryout.text = (tryout.text + stroke.text).slice(-400)
  }
  tryout.hear?.(stroke, at)
}

/**
 * Open or close the typing test. Where the browser can, the change is one
 * view transition, so what both views share (the keyboard's picture, named
 * `keyboard` in their styles) moves to its new place instead of being swapped.
 */
export function setExpanded(tryout: Tryout, on: boolean): void {
  const change = async () => {
    tryout.expanded = on
    await nextTick()
  }
  const still = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches
  if (typeof document === 'undefined' || !document.startViewTransition || still) void change()
  else document.startViewTransition(change)
}

export function clearTryout(tryout: Tryout): void {
  tryout.text = ''
  clearLog(tryout.log)
}

export const loseHeld = (tryout: Tryout) => logLoseHeld(tryout.log)

/** Feed a tryout from the virtual keyboards whose sessions `ids` names. Returns how to stop. */
export function hearVirtual(tryout: Tryout, ids: () => (string | undefined)[]): () => void {
  const readers = new Map<string, ReportReader>()
  return onHostEvent((session, event, at) => {
    if (!ids().includes(session.id)) return
    const reader = readers.get(session.id) ?? new ReportReader()
    readers.set(session.id, reader)
    // The two halves of a split are two keyboards to the computer: the same key on each is two keys.
    for (const stroke of reader.read(event)) takeStroke(tryout, { ...stroke, key: `${session.id}:${stroke.key}` }, at, true)
  })
}
