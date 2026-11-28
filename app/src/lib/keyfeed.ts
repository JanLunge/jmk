// Key presses and releases, from the two places they come from: the reports a
// virtual keyboard sends its computer, and the browser's key events for a
// real one.

import { charFor, keyLabel, MEDIA_KEYS, MOD_BITS, modsLabel } from './keycodes'
import { ALTGR_BIT, charsOf } from './language'
import type { HostEvent } from './protocol'

export type StrokeKind = 'char' | 'backspace' | 'tab' | 'escape' | 'enter' | 'other'

export interface Stroke {
  /** What tells this key from the others. */
  key: string
  down: boolean
  label: string
  kind: StrokeKind
  /** What it types, where that is known from the key alone. */
  text?: string
  /** A Backspace that takes a whole word (with Ctrl or Alt). */
  word?: boolean
}

const SHIFT_BITS = 0x22
const KINDS = new Map<number, StrokeKind>([[0x2a, 'backspace'], [0x2b, 'tab'], [0x29, 'escape'], [0x28, 'enter']])

/** The character a key types with these modifiers held on a computer set to the chosen language. */
export function typedChar(usage: number, mods: number): string | undefined {
  const shift = (mods & SHIFT_BITS) !== 0
  const rest = mods & ~SHIFT_BITS
  if (rest === 0) return charFor(usage, shift)
  if (rest !== ALTGR_BIT) return undefined
  const chars = charsOf(usage)
  return (shift ? chars?.altshift : chars?.altgr) || undefined
}

/**
 * Reads the reports of one keyboard and says which keys went down and up
 * with each. A report names everything held, so a key is new when the report
 * before did not have it.
 */
export class ReportReader {
  private keys: number[] = []
  private mods = 0
  private consumer = 0

  read(event: HostEvent): Stroke[] {
    const out: Stroke[] = []
    if ('Consumer' in event) {
      const label = (usage: number) => MEDIA_KEYS.find((key) => key.usage === usage)?.label ?? 'Media'
      if (this.consumer && this.consumer !== event.Consumer) out.push({ key: `c${this.consumer}`, down: false, label: label(this.consumer), kind: 'other' })
      if (event.Consumer && this.consumer !== event.Consumer) out.push({ key: `c${event.Consumer}`, down: true, label: label(event.Consumer), kind: 'other' })
      this.consumer = event.Consumer
      return out
    }
    const { mods, keys } = event.Keyboard
    const modifier = (bit: number, down: boolean): Stroke => ({ key: `m${bit}`, down, label: MOD_BITS.find((each) => each.bit === bit)!.label, kind: 'other' })
    // Modifiers go down before the keys they change, and come up after them.
    for (const { bit } of MOD_BITS) if (mods & bit && !(this.mods & bit)) out.push(modifier(bit, true))
    for (const usage of this.keys) if (!keys.includes(usage)) out.push({ key: `k${usage}`, down: false, label: keyLabel(usage), kind: KINDS.get(usage) ?? 'other' })
    for (const usage of keys) {
      if (usage === 0 || this.keys.includes(usage)) continue
      const text = typedChar(usage, mods)
      const others = mods & ~SHIFT_BITS & (text ? ~ALTGR_BIT : ~0)
      const name = text === ' ' ? 'Space' : (text ?? keyLabel(usage))
      const kind = KINDS.get(usage) ?? (text ? 'char' : 'other')
      out.push({
        key: `k${usage}`,
        down: true,
        label: others ? `${modsLabel(others).replaceAll('+', ' + ')} + ${name}` : name,
        kind: others && kind === 'char' ? 'other' : kind,
        // With Ctrl, Alt or GUI held a key is a shortcut, not a character.
        text: others ? undefined : text,
        // Ctrl or Alt, and no GUI.
        word: kind === 'backspace' && (mods & 0x55) !== 0 && (mods & 0x88) === 0,
      })
    }
    for (const { bit } of MOD_BITS) if (!(mods & bit) && this.mods & bit) out.push(modifier(bit, false))
    this.keys = [...keys]
    this.mods = mods
    return out
  }
}

const NAMES: Record<string, string> = {
  ' ': 'Space', Escape: 'Esc', Backspace: 'Bksp', Delete: 'Del', ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→',
  Control: 'Ctrl', Meta: 'GUI', AudioVolumeUp: 'Vol+', AudioVolumeDown: 'Vol−', AudioVolumeMute: 'Mute',
  MediaPlayPause: 'Play', MediaTrackNext: 'Next', MediaTrackPrevious: 'Prev', MediaStop: 'Stop',
}
const DOM_KINDS: Record<string, StrokeKind> = { Backspace: 'backspace', Tab: 'tab', Escape: 'escape', Enter: 'enter' }

/** What the browser says of a key going down or up. What it types is left to the field it is typed into: an accent key types nothing by itself. */
export function domStroke(event: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'altKey' | 'metaKey'>, down: boolean): Stroke {
  const name = NAMES[event.key] ?? (event.key === 'Unidentified' || event.key === 'Dead' || event.key === 'Process' ? event.code || event.key : event.key)
  const mods = [event.ctrlKey && name !== 'Ctrl' && 'Ctrl', event.altKey && name !== 'Alt' && 'Alt', event.metaKey && name !== 'GUI' && 'GUI'].filter(Boolean)
  const kind = DOM_KINDS[event.key] ?? ([...event.key].length === 1 && !event.ctrlKey && !event.metaKey ? 'char' : 'other')
  return {
    // The place of the key, where the browser knows it: the same for press and release whatever Shift did in between.
    key: event.code || event.key,
    down,
    label: [...mods, name].join(' + '),
    kind,
    word: kind === 'backspace' && (event.ctrlKey || event.altKey) && !event.metaKey,
  }
}
