// What can be put on a key, with the names people know them by.

import type { Action, Basic, MacroStep } from './protocol'

export interface KeyInfo {
  usage: number
  /** Short text for a keycap. */
  label: string
  name: string
}

export interface KeyGroup {
  title: string
  keys: KeyInfo[]
}

const k = (usage: number, label: string, name = label): KeyInfo => ({ usage, label, name })
const range = (first: number, labels: string[], name: (l: string) => string = (l) => l) =>
  labels.map((label, i) => k(first + i, label, name(label)))

export const KEY_GROUPS: KeyGroup[] = [
  { title: 'Letters', keys: range(0x04, [...'ABCDEFGHIJKLMNOPQRSTUVWXYZ']) },
  { title: 'Numbers', keys: range(0x1e, [...'1234567890']) },
  {
    title: 'Punctuation',
    keys: [
      k(0x2d, '-', 'Minus'), k(0x2e, '=', 'Equals'), k(0x2f, '[', 'Left bracket'), k(0x30, ']', 'Right bracket'),
      k(0x31, '\\', 'Backslash'), k(0x33, ';', 'Semicolon'), k(0x34, "'", 'Quote'), k(0x35, '`', 'Grave'),
      k(0x36, ',', 'Comma'), k(0x37, '.', 'Period'), k(0x38, '/', 'Slash'), k(0x32, '#~', 'Non-US hash'),
      k(0x64, '\\|', 'Non-US backslash'),
    ],
  },
  {
    title: 'Editing',
    keys: [
      k(0x28, 'Enter'), k(0x29, 'Esc', 'Escape'), k(0x2a, 'Bksp', 'Backspace'), k(0x2b, 'Tab'), k(0x2c, 'Space'),
      k(0x4c, 'Del', 'Delete'), k(0x49, 'Ins', 'Insert'), k(0x39, 'Caps', 'Caps Lock'), k(0x46, 'PrtSc', 'Print Screen'),
      k(0x47, 'ScrLk', 'Scroll Lock'), k(0x48, 'Pause'), k(0x65, 'Menu', 'Application menu'),
    ],
  },
  {
    title: 'Navigation',
    keys: [
      k(0x52, '↑', 'Up'), k(0x51, '↓', 'Down'), k(0x50, '←', 'Left'), k(0x4f, '→', 'Right'), k(0x4a, 'Home'),
      k(0x4d, 'End'), k(0x4b, 'PgUp', 'Page Up'), k(0x4e, 'PgDn', 'Page Down'),
    ],
  },
  {
    title: 'Modifiers',
    keys: [
      k(0xe0, 'Ctrl', 'Left Control'), k(0xe1, 'Shift', 'Left Shift'), k(0xe2, 'Alt', 'Left Alt'),
      k(0xe3, 'GUI', 'Left GUI (Win / Cmd)'), k(0xe4, 'RCtrl', 'Right Control'), k(0xe5, 'RShift', 'Right Shift'),
      k(0xe6, 'RAlt', 'Right Alt (AltGr)'), k(0xe7, 'RGUI', 'Right GUI'),
    ],
  },
  {
    title: 'Function keys',
    keys: [
      ...range(0x3a, ['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12']),
      ...range(0x68, ['F13', 'F14', 'F15', 'F16', 'F17', 'F18', 'F19', 'F20', 'F21', 'F22', 'F23', 'F24']),
    ],
  },
  {
    title: 'Number pad',
    keys: [
      k(0x53, 'NumLk', 'Num Lock'), k(0x54, 'N/', 'Numpad divide'), k(0x55, 'N*', 'Numpad multiply'),
      k(0x56, 'N-', 'Numpad minus'), k(0x57, 'N+', 'Numpad plus'), k(0x58, 'NEnt', 'Numpad Enter'),
      ...range(0x59, ['N1', 'N2', 'N3', 'N4', 'N5', 'N6', 'N7', 'N8', 'N9', 'N0'], (l) => `Numpad ${l[1]}`),
      k(0x63, 'N.', 'Numpad period'),
    ],
  },
]

/** Consumer-page usages (media keys). */
export const MEDIA_KEYS: KeyInfo[] = [
  k(0xcd, '⏯', 'Play / Pause'), k(0xb5, '⏭', 'Next track'), k(0xb6, '⏮', 'Previous track'), k(0xb7, '⏹', 'Stop'),
  k(0xe2, 'Mute'), k(0xe9, 'Vol+', 'Volume up'), k(0xea, 'Vol−', 'Volume down'), k(0x6f, 'Bri+', 'Brightness up'),
  k(0x70, 'Bri−', 'Brightness down'), k(0x192, 'Calc', 'Calculator'), k(0x18a, 'Mail'), k(0x223, 'WWW', 'Browser home'),
  k(0x224, 'Back', 'Browser back'), k(0x225, 'Fwd', 'Browser forward'), k(0x227, 'Refr', 'Browser refresh'),
]

const KEY_BY_USAGE = new Map(KEY_GROUPS.flatMap((g) => g.keys).map((key) => [key.usage, key]))
const MEDIA_BY_USAGE = new Map(MEDIA_KEYS.map((key) => [key.usage, key]))

export const MOD_BITS = [
  { bit: 0x01, label: 'Ctrl' }, { bit: 0x02, label: 'Shift' }, { bit: 0x04, label: 'Alt' }, { bit: 0x08, label: 'GUI' },
  { bit: 0x10, label: 'RCtrl' }, { bit: 0x20, label: 'RShift' }, { bit: 0x40, label: 'RAlt' }, { bit: 0x80, label: 'RGUI' },
]

export function modsLabel(mods: number): string {
  const names = MOD_BITS.filter((m) => mods & m.bit).map((m) => m.label)
  return names.length ? names.join('+') : 'no modifier'
}

export function keyLabel(usage: number): string {
  return KEY_BY_USAGE.get(usage)?.label ?? `0x${usage.toString(16)}`
}

export function keyName(usage: number): string {
  return KEY_BY_USAGE.get(usage)?.name ?? `Key 0x${usage.toString(16)}`
}

/** What to print on a keycap. `kind` picks the colour. */
export interface Legend {
  main: string
  /** Second line, e.g. the hold action of a hold-tap. */
  sub?: string
  kind: 'key' | 'mod' | 'layer' | 'media' | 'macro' | 'empty' | 'through' | 'special'
}

export function basicLegend(basic: Basic): Legend {
  if (basic === 'None') return { main: '', kind: 'empty' }
  if ('Key' in basic) return { main: keyLabel(basic.Key), kind: basic.Key >= 0xe0 ? 'mod' : 'key' }
  if ('Mods' in basic) return { main: modsLabel(basic.Mods), kind: 'mod' }
  if ('ModKey' in basic) return { main: `${modsLabel(basic.ModKey.mods)}+${keyLabel(basic.ModKey.key)}`, kind: 'key' }
  if ('Consumer' in basic) return { main: MEDIA_BY_USAGE.get(basic.Consumer)?.label ?? 'Media', kind: 'media' }
  if ('MomentaryLayer' in basic) return { main: `L${basic.MomentaryLayer}`, sub: 'hold', kind: 'layer' }
  if ('ToggleLayer' in basic) return { main: `L${basic.ToggleLayer}`, sub: 'toggle', kind: 'layer' }
  if ('ToLayer' in basic) return { main: `L${basic.ToLayer}`, sub: 'go to', kind: 'layer' }
  if ('OneShotMods' in basic) return { main: modsLabel(basic.OneShotMods), sub: 'one-shot', kind: 'mod' }
  if ('Macro' in basic) return { main: `M${basic.Macro + 1}`, sub: 'macro', kind: 'macro' }
  return { main: `#${basic.Custom}`, sub: 'custom', kind: 'special' }
}

export function legend(action: Action): Legend {
  if (action === 'Transparent') return { main: '▽', kind: 'through' }
  if ('Basic' in action) return basicLegend(action.Basic)
  const tap = basicLegend(action.HoldTap.tap)
  const hold = basicLegend(action.HoldTap.hold)
  return { main: tap.main, sub: `hold: ${hold.main}`, kind: tap.kind === 'empty' ? hold.kind : tap.kind }
}

export function describeBasic(basic: Basic): string {
  if (basic === 'None') return 'Nothing'
  if ('Key' in basic) return keyName(basic.Key)
  if ('Mods' in basic) return modsLabel(basic.Mods)
  if ('ModKey' in basic) return `${modsLabel(basic.ModKey.mods)} + ${keyName(basic.ModKey.key)}`
  if ('Consumer' in basic) return MEDIA_BY_USAGE.get(basic.Consumer)?.name ?? 'Media key'
  if ('MomentaryLayer' in basic) return `Layer ${basic.MomentaryLayer} while held`
  if ('ToggleLayer' in basic) return `Toggle layer ${basic.ToggleLayer}`
  if ('ToLayer' in basic) return `Switch to layer ${basic.ToLayer}`
  if ('OneShotMods' in basic) return `${modsLabel(basic.OneShotMods)} for the next key`
  if ('Macro' in basic) return `Macro ${basic.Macro + 1}`
  return `Custom action ${basic.Custom}`
}

export function describeAction(action: Action): string {
  if (action === 'Transparent') return 'See-through: uses the key from the layer below'
  if ('Basic' in action) return describeBasic(action.Basic)
  return `${describeBasic(action.HoldTap.tap)} when tapped, ${describeBasic(action.HoldTap.hold)} when held`
}

export const key = (usage: number): Action => ({ Basic: { Key: usage } })
export const NONE: Action = { Basic: 'None' }

// --- Text for macros (US layout) -------------------------------------------

const SHIFT = 0xe1
// Index = HID usage - 0x04. \u0000 fills usage 0x32, which US layouts lack.
const PLAIN = "abcdefghijklmnopqrstuvwxyz1234567890\n\u001b\b\t -=[]\\\u0000;'`,./"
const SHIFTED = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ!@#$%^&*()\n\u001b\b\t _+{}|\u0000:"~<>?'

/** Macro steps that type `text` on a computer set to a US layout. Unknown characters are skipped. */
export function textToSteps(text: string): MacroStep[] {
  const steps: MacroStep[] = []
  let shifted = false
  const setShift = (want: boolean) => {
    if (want !== shifted) steps.push(want ? { Press: SHIFT } : { Release: SHIFT })
    shifted = want
  }
  for (const char of text) {
    let index = PLAIN.indexOf(char)
    const needsShift = index < 0
    if (needsShift) index = SHIFTED.indexOf(char)
    if (index < 0) continue
    setShift(needsShift)
    steps.push({ Tap: 0x04 + index })
  }
  setShift(false)
  return steps
}

/** The reverse of `textToSteps` where possible, for showing a macro. */
export function describeStep(step: MacroStep): string {
  if ('Tap' in step) return `Tap ${keyName(step.Tap)}`
  if ('Press' in step) return `Hold ${keyName(step.Press)}`
  if ('Release' in step) return `Release ${keyName(step.Release)}`
  return `Wait ${step.DelayMs} ms`
}
