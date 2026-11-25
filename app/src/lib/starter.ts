// The first picture and keymap of a keyboard that was just set up: rows from
// the order the keys were pressed in, and letters from a known arrangement.
import { hasKey, NO_KEY, UNIT } from './layout'
import type { Action, KeyGeometry } from './protocol'

/** The three letter rows of an arrangement, ten keys each, top row first. */
export interface Alphas {
  id: string
  name: string
  rows: [string, string, string]
}

export const ALPHAS: Alphas[] = [
  { id: 'qwerty', name: 'QWERTY', rows: ['qwertyuiop', 'asdfghjkl;', 'zxcvbnm,./'] },
  { id: 'colemak-dh', name: 'Colemak-DH', rows: ['qwfpbjluy;', 'arstgmneio', 'zxcdvkh,./'] },
  { id: 'colemak', name: 'Colemak', rows: ['qwfpgjluy;', 'arstdhneio', 'zxcvbkm,./'] },
  { id: 'dvorak', name: 'Dvorak', rows: ["',.pyfgcrl", 'aoeuidhtns', ';qjkxbmwvz'] },
]
export const ALPHA_ROW_NAMES = ['Top row', 'Home row', 'Bottom row']
export const ALPHA_ROW_LENGTH = 10

const PUNCTUATION: Record<string, number> = { ';': 0x33, "'": 0x34, ',': 0x36, '.': 0x37, '/': 0x38 }
/** The key code that types this character on a computer set to US English. */
export const usageOf = (char: string): number => PUNCTUATION[char] ?? 0x04 + char.charCodeAt(0) - 97

/** Keys in press order, cut into rows after each key that ends one. */
export function splitRows(count: number, ends: number[]): number[][] {
  const rows: number[][] = [[]]
  for (let n = 0; n < count; n++) {
    rows[rows.length - 1].push(n)
    if (ends.includes(n) && n < count - 1) rows.push([])
  }
  return count ? rows : []
}

/**
 * A picture from rows of key positions: one unit per key, a row per line.
 * Shorter rows line up on the left, or on the right for the left half of a
 * split, whose thumb keys sit towards the middle.
 */
export function rowLayout(rows: number[][], count: number, align: 'left' | 'right' = 'left'): KeyGeometry[] {
  const layout: KeyGeometry[] = Array.from({ length: count }, () => ({ ...NO_KEY }))
  const longest = Math.max(0, ...rows.map((row) => row.length))
  rows.forEach((row, line) =>
    row.forEach((pos, index) => {
      const column = align === 'right' ? longest - row.length + index : index
      if (pos < count) layout[pos] = { x: column * UNIT, y: line * UNIT, w: UNIT, h: UNIT, r: 0 }
    }),
  )
  return layout
}

/** `pos` and the keys to its right in the same row of the picture, nearest first. */
export function rowFrom(layout: KeyGeometry[], pos: number, limit = ALPHA_ROW_LENGTH): number[] {
  const here = layout[pos]
  if (!hasKey(here)) return []
  const middle = here.y + here.h / 2
  return layout
    .flatMap((key, index) => (hasKey(key) && key.x >= here.x && Math.abs(key.y + key.h / 2 - middle) < here.h / 2 ? [{ index, x: key.x }] : []))
    .sort((a, b) => a.x - b.x)
    .slice(0, limit)
    .map((key) => key.index)
}

/** The letters of an arrangement on the keys chosen for its rows. */
export function letterActions(rows: number[][], alphas: Alphas): Map<number, Action> {
  const actions = new Map<number, Action>()
  rows.slice(0, 3).forEach((row, line) =>
    row.slice(0, ALPHA_ROW_LENGTH).forEach((pos, index) => actions.set(pos, { Basic: { Key: usageOf(alphas.rows[line][index]) } })),
  )
  return actions
}

/** The arrangement whose letters a layer already has on these rows, as far as one fits; the first one otherwise. */
export function guessAlphas(layer: Action[] | undefined, rows: number[][]): Alphas {
  const typed = (pos: number): number | undefined => {
    const action = layer?.[pos]
    if (!action || typeof action !== 'object') return undefined
    const basic = 'HoldTap' in action ? action.HoldTap.tap : action.Basic
    return typeof basic === 'object' && 'Key' in basic ? basic.Key : undefined
  }
  const score = (alphas: Alphas) => [...letterActions(rows, alphas)].filter(([pos, action]) => typed(pos) === ((action as { Basic: { Key: number } }).Basic.Key)).length
  return ALPHAS.reduce((best, alphas) => (score(alphas) > score(best) ? alphas : best), ALPHAS[0])
}
