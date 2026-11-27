// What can be put on a key, with the names people know them by.

import { ALTGR_BIT, charsOf, heldName, keyFor, language, US } from './language'
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
    // Keys that only keyboards for some languages have. Nothing on a computer set to another one.
    title: 'International',
    keys: [
      k(0x87, 'Ro', 'International 1 (Japanese Ro, Brazilian / ?)'), k(0x88, 'Kana', 'International 2 (Japanese Katakana / Hiragana)'),
      k(0x89, '¥', 'International 3 (Japanese Yen)'), k(0x8a, 'Henk', 'International 4 (Japanese Henkan)'),
      k(0x8b, 'Mhen', 'International 5 (Japanese Muhenkan)'), k(0x90, '한/영', 'Language 1 (Korean Hangul / English, Mac Kana)'),
      k(0x91, '한자', 'Language 2 (Korean Hanja, Mac Eisu)'), k(0x85, 'N,', 'Numpad comma (Brazilian)'), k(0x67, 'N=', 'Numpad equals'),
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

/** One key of the picture of an ordinary keyboard: its usage (or undefined for a gap) and width in key units. */
export type BoardKey = [usage: number | undefined, width: number]
const row = (...keys: (number | BoardKey)[]): BoardKey[] => keys.map((key) => (typeof key === 'number' ? [key, 1] : key))
const gap = (width: number): BoardKey => [undefined, width]
const span = (first: number, count: number) => [...Array(count).keys()].map((i) => first + i)

/**
 * An ordinary (ANSI) keyboard, for picking keys where the hand expects them:
 * the main block, the navigation keys and the number pad, row by row.
 */
export const ANSI_BLOCKS: { id: string; rows: BoardKey[][] }[] = [
  {
    id: 'main',
    rows: [
      row(0x29, gap(1), ...span(0x3a, 4), gap(0.5), ...span(0x3e, 4), gap(0.5), ...span(0x42, 4)),
      row(0x35, ...span(0x1e, 10), 0x2d, 0x2e, [0x2a, 2]),
      row([0x2b, 1.5], 0x14, 0x1a, 0x08, 0x15, 0x17, 0x1c, 0x18, 0x0c, 0x12, 0x13, 0x2f, 0x30, [0x31, 1.5]),
      row([0x39, 1.75], 0x04, 0x16, 0x07, 0x09, 0x0a, 0x0b, 0x0d, 0x0e, 0x0f, 0x33, 0x34, [0x28, 2.25]),
      row([0xe1, 2.25], 0x1d, 0x1b, 0x06, 0x19, 0x05, 0x11, 0x10, 0x36, 0x37, 0x38, [0xe5, 2.75]),
      row([0xe0, 1.25], [0xe3, 1.25], [0xe2, 1.25], [0x2c, 6.25], [0xe6, 1.25], [0xe7, 1.25], [0x65, 1.25], [0xe4, 1.25]),
    ],
  },
  {
    id: 'nav',
    rows: [row(0x46, 0x47, 0x48), row(0x49, 0x4a, 0x4b), row(0x4c, 0x4d, 0x4e), row(gap(3)), row(gap(1), 0x52), row(0x50, 0x51, 0x4f)],
  },
  {
    id: 'numpad',
    rows: [row(gap(4)), row(0x53, 0x54, 0x55, 0x56), row(0x5f, 0x60, 0x61, 0x57), row(0x5c, 0x5d, 0x5e), row(0x59, 0x5a, 0x5b, 0x58), row([0x62, 2], 0x63)],
  },
]

/** The same picture for an ISO keyboard: one more key next to a shorter left Shift. */
const ISO_BLOCKS: typeof ANSI_BLOCKS = ANSI_BLOCKS.map((block) =>
  block.id !== 'main' ? block : { ...block, rows: block.rows.map((keys, n) => (n === 4 ? [[0xe1, 1.25], [0x64, 1], ...keys.slice(1)] : keys)) },
)

/** The picture of an ordinary keyboard as the chosen language has it. */
export const boardBlocks = () => (language().iso ? ISO_BLOCKS : ANSI_BLOCKS)

/** Keys an ordinary keyboard does not have. */
export const EXTRA_KEYS: KeyInfo[] = [
  ...range(0x68, ['F13', 'F14', 'F15', 'F16', 'F17', 'F18', 'F19', 'F20', 'F21', 'F22', 'F23', 'F24']),
  k(0x32, '#~', 'Non-US hash'),
  k(0x64, '\\|', 'Non-US backslash'),
  ...[0x87, 0x88, 0x89, 0x8a, 0x8b, 0x90, 0x91, 0x85, 0x67].map((usage) => KEY_GROUPS.flatMap((g) => g.keys).find((key) => key.usage === usage)!),
]

/** Consumer-page usages (media keys). */
export const MEDIA_KEYS: KeyInfo[] = [
  k(0xcd, '⏯', 'Play / Pause'), k(0xb5, '⏭', 'Next track'), k(0xb6, '⏮', 'Previous track'), k(0xb7, '⏹', 'Stop'),
  k(0xe2, 'Mute'), k(0xe9, 'Vol+', 'Volume up'), k(0xea, 'Vol−', 'Volume down'), k(0x6f, 'Bri+', 'Brightness up'),
  k(0x70, 'Bri−', 'Brightness down'), k(0x192, 'Calc', 'Calculator'), k(0x18a, 'Mail'), k(0x223, 'WWW', 'Browser home'),
  k(0x224, 'Back', 'Browser back'), k(0x225, 'Fwd', 'Browser forward'), k(0x227, 'Refr', 'Browser refresh'),
]

/**
 * The symbol printed on such a key, for keys that have one people know.
 * The word stays next to it wherever there is room, for whoever does not.
 */
const ICONS = new Map<number, string>([
  [0x28, '⏎'], [0x29, '⎋'], [0x2a, '⌫'], [0x2b, '⇥'], [0x2c, '␣'], [0x4c, '⌦'], [0x39, '⇪'],
  [0x4a, '↖'], [0x4d, '↘'], [0x4b, '⇞'], [0x4e, '⇟'], [0x65, '☰'],
  [0xe0, '⌃'], [0xe1, '⇧'], [0xe2, '⌥'], [0xe3, '⌘'], [0xe4, '⌃'], [0xe5, '⇧'], [0xe6, '⌥'], [0xe7, '⌘'],
])

/** The symbol for a key, if it has one. */
export const keyIcon = (usage: number): string | undefined => ICONS.get(usage)

const KEY_BY_USAGE = new Map(KEY_GROUPS.flatMap((g) => g.keys).map((key) => [key.usage, key]))
const MEDIA_BY_USAGE = new Map(MEDIA_KEYS.map((key) => [key.usage, key]))

/** HID usage for a `KeyboardEvent.code`, i.e. for the physical key whatever the system's language. */
const USAGE_BY_CODE = new Map<string, number>([
  ...[...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'].map((letter, i): [string, number] => [`Key${letter}`, 0x04 + i]),
  ...[...'123456789'].map((digit, i): [string, number] => [`Digit${digit}`, 0x1e + i]),
  ...[...'123456789'].map((digit, i): [string, number] => [`Numpad${digit}`, 0x59 + i]),
  ...[...Array(12).keys()].map((i): [string, number] => [`F${i + 1}`, 0x3a + i]),
  ...[...Array(12).keys()].map((i): [string, number] => [`F${i + 13}`, 0x68 + i]),
  ['Digit0', 0x27], ['Enter', 0x28], ['Escape', 0x29], ['Backspace', 0x2a], ['Tab', 0x2b], ['Space', 0x2c],
  ['Minus', 0x2d], ['Equal', 0x2e], ['BracketLeft', 0x2f], ['BracketRight', 0x30], ['Backslash', 0x31],
  ['IntlHash', 0x32], ['Semicolon', 0x33], ['Quote', 0x34], ['Backquote', 0x35], ['Comma', 0x36], ['Period', 0x37],
  ['Slash', 0x38], ['CapsLock', 0x39], ['PrintScreen', 0x46], ['ScrollLock', 0x47], ['Pause', 0x48],
  ['Insert', 0x49], ['Home', 0x4a], ['PageUp', 0x4b], ['Delete', 0x4c], ['End', 0x4d], ['PageDown', 0x4e],
  ['ArrowRight', 0x4f], ['ArrowLeft', 0x50], ['ArrowDown', 0x51], ['ArrowUp', 0x52], ['NumLock', 0x53],
  ['NumpadDivide', 0x54], ['NumpadMultiply', 0x55], ['NumpadSubtract', 0x56], ['NumpadAdd', 0x57],
  ['NumpadEnter', 0x58], ['Numpad0', 0x62], ['NumpadDecimal', 0x63], ['IntlBackslash', 0x64], ['ContextMenu', 0x65],
  ['ControlLeft', 0xe0], ['ShiftLeft', 0xe1], ['AltLeft', 0xe2], ['MetaLeft', 0xe3],
  ['ControlRight', 0xe4], ['ShiftRight', 0xe5], ['AltRight', 0xe6], ['MetaRight', 0xe7],
])

export function usageForCode(code: string): number | undefined {
  return USAGE_BY_CODE.get(code)
}

export const isModifierUsage = (usage: number) => usage >= 0xe0 && usage <= 0xe7

export const MOD_BITS = [
  { bit: 0x01, label: 'Ctrl' }, { bit: 0x02, label: 'Shift' }, { bit: 0x04, label: 'Alt' }, { bit: 0x08, label: 'GUI' },
  { bit: 0x10, label: 'RCtrl' }, { bit: 0x20, label: 'RShift' }, { bit: 0x40, label: 'RAlt' }, { bit: 0x80, label: 'RGUI' },
]

export function modsLabel(mods: number): string {
  const names = MOD_BITS.filter((m) => mods & m.bit).map((m) => m.label)
  return names.length ? names.join('+') : 'no modifier'
}

const usLabel = (usage: number) => KEY_BY_USAGE.get(usage)?.label ?? `0x${usage.toString(16)}`
const isLetter = (char: string) => char.toLowerCase() !== char.toUpperCase()

/**
 * What a key types on a computer set to the chosen language, if that differs
 * from what its key code is called on a US keyboard.
 */
function local(usage: number): { label: string; name: string } | undefined {
  const lang = language()
  if (lang === US) return undefined
  const chars = charsOf(usage, lang)
  if (!chars?.plain) return undefined
  const { plain, shifted } = chars
  const us = charsOf(usage, US)
  if (us?.plain === plain && us.shifted === shifted) return undefined
  // A letter key types its capital with Shift; é on the French 2 key is not one.
  const letter = isLetter(plain) && shifted === plain.toUpperCase()
  // A row of digits under Shift (French) is still the number row to whoever looks for a key.
  const label = letter ? shifted : /\d/.test(shifted) ? shifted : plain
  const both = letter || !shifted ? label : `${plain} and ${shifted}`
  return { label, name: `${both} (US: ${KEY_BY_USAGE.get(usage)?.name ?? usLabel(usage)})` }
}

/**
 * The character a key types with Shift or AltGr held, in the chosen language:
 * what `Shift+2` or `RAlt+2` is to whoever put it on a key. Capital letters are left as they are.
 */
export function modKeyChar(mods: number, usage: number): string | undefined {
  const chars = charsOf(usage)
  if (!chars) return undefined
  const shift = mods === 0x02 || mods === 0x20
  const char = shift ? chars.shifted : mods === ALTGR_BIT ? chars.altgr : mods === (ALTGR_BIT | 0x02) ? chars.altshift : ''
  return char && !(shift && isLetter(chars.plain) && char === chars.plain.toUpperCase()) ? char : undefined
}

export function keyLabel(usage: number): string {
  return local(usage)?.label ?? usLabel(usage)
}

export function keyName(usage: number): string {
  return local(usage)?.name ?? KEY_BY_USAGE.get(usage)?.name ?? `Key 0x${usage.toString(16)}`
}

/** What to print on a keycap. `kind` picks the colour. */
export interface Legend {
  main: string
  /** Second line, e.g. the hold action of a hold-tap. */
  sub?: string
  kind: 'key' | 'mod' | 'layer' | 'media' | 'macro' | 'empty' | 'through' | 'special'
  /** The layer this key leads to, if it is a layer key. */
  layer?: number
  /** The key in words, where `main` is its symbol. */
  word?: string
}

export function basicLegend(basic: Basic): Legend {
  if (basic === 'None') return { main: '', kind: 'empty' }
  if ('Key' in basic) {
    const kind = basic.Key >= 0xe0 ? 'mod' : 'key'
    const icon = keyIcon(basic.Key)
    // The symbol, with the word small below it.
    return icon ? { main: icon, sub: keyLabel(basic.Key), word: keyLabel(basic.Key), kind } : { main: keyLabel(basic.Key), kind }
  }
  if ('Mods' in basic) return { main: modsLabel(basic.Mods), kind: 'mod' }
  if ('ModKey' in basic) {
    const { mods, key } = basic.ModKey
    const char = modKeyChar(mods, key)
    const keys = `${heldName(mods) ?? modsLabel(mods)}+${keyLabel(key)}`
    // The character itself, with how it is typed small below it.
    return char ? { main: char, sub: keys, word: char, kind: 'key' } : { main: keys, kind: 'key' }
  }
  if ('Consumer' in basic) return { main: MEDIA_BY_USAGE.get(basic.Consumer)?.label ?? 'Media', kind: 'media' }
  if ('MomentaryLayer' in basic) return { main: `L${basic.MomentaryLayer}`, sub: 'hold', kind: 'layer', layer: basic.MomentaryLayer }
  if ('ToggleLayer' in basic) return { main: `L${basic.ToggleLayer}`, sub: 'toggle', kind: 'layer', layer: basic.ToggleLayer }
  if ('ToLayer' in basic) return { main: `L${basic.ToLayer}`, sub: 'go to', kind: 'layer', layer: basic.ToLayer }
  if ('OneShotMods' in basic) return { main: modsLabel(basic.OneShotMods), sub: 'one-shot', kind: 'mod' }
  if ('Macro' in basic) return { main: `M${basic.Macro + 1}`, sub: 'macro', kind: 'macro' }
  return { main: `#${basic.Custom}`, sub: 'custom', kind: 'special' }
}

export function legend(action: Action): Legend {
  if (action === 'Transparent') return { main: '▽', kind: 'through' }
  if ('Basic' in action) return basicLegend(action.Basic)
  const tap = basicLegend(action.HoldTap.tap)
  const hold = basicLegend(action.HoldTap.hold)
  return { main: tap.main, sub: `hold: ${hold.word ?? hold.main}`, kind: tap.kind === 'empty' ? hold.kind : tap.kind, layer: hold.layer ?? tap.layer }
}

export function describeBasic(basic: Basic): string {
  if (basic === 'None') return 'Nothing'
  if ('Key' in basic) return keyName(basic.Key)
  if ('Mods' in basic) return modsLabel(basic.Mods)
  if ('ModKey' in basic) {
    const { mods, key } = basic.ModKey
    const char = modKeyChar(mods, key)
    const held = heldName(mods) ?? modsLabel(mods)
    return char ? `${char}, typed as ${held} + ${keyLabel(key)}` : `${held} + ${keyName(key)}`
  }
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

// --- Text for macros --------------------------------------------------------

const SHIFT = 0xe1
const ALTGR = 0xe6
/** Characters that are the same key in every language. */
const FIXED = new Map<string, number>([['\n', 0x28], ['\t', 0x2b], [' ', 0x2c]])

/**
 * Macro steps that type `text` on a computer set to the chosen language.
 * Characters that language has no key for, or needs an accent key for, are skipped.
 */
export function textToSteps(text: string): MacroStep[] {
  const steps: MacroStep[] = []
  const held = new Map([[SHIFT, false], [ALTGR, false]])
  const hold = (key: number, want: boolean) => {
    if (want !== held.get(key)) steps.push(want ? { Press: key } : { Release: key })
    held.set(key, want)
  }
  for (const char of text) {
    const fixed = FIXED.get(char)
    const found = fixed === undefined ? keyFor(char) : { usage: fixed, mods: 0 }
    if (!found) continue
    hold(SHIFT, (found.mods & 0x02) !== 0)
    hold(ALTGR, (found.mods & ALTGR_BIT) !== 0)
    steps.push({ Tap: found.usage })
  }
  hold(SHIFT, false)
  hold(ALTGR, false)
  return steps
}

/** The reverse of `textToSteps` where possible, for showing a macro. */
export function describeStep(step: MacroStep): string {
  if ('Tap' in step) return `Tap ${keyName(step.Tap)}`
  if ('Press' in step) return `Hold ${keyName(step.Press)}`
  if ('Release' in step) return `Release ${keyName(step.Release)}`
  return `Wait ${step.DelayMs} ms`
}

/** The character a key produces on a computer set to the chosen language, if it is a printable one. */
export function charFor(usage: number, shifted: boolean): string | undefined {
  if (usage === 0x2c) return ' '
  const chars = charsOf(usage)
  return (shifted ? chars?.shifted : chars?.plain) || undefined
}
