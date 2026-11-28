// A colour per layer, so a layer and the keys that lead to it are found at a glance.

import { legend, type Legend } from './keycodes'
import type { Action, Basic } from './protocol'
import type { Session } from './session'

/** Colour numbers as stored on the keyboard start at 1; 0 means none was chosen. */
export const LAYER_COLORS: { name: string; hex: string }[] = [
  { name: 'Amber', hex: '#f59e0b' },
  { name: 'Blue', hex: '#3b82f6' },
  { name: 'Green', hex: '#10b981' },
  { name: 'Red', hex: '#ef4444' },
  { name: 'Purple', hex: '#a855f7' },
  { name: 'Cyan', hex: '#06b6d4' },
  { name: 'Pink', hex: '#ec4899' },
  { name: 'Lime', hex: '#84cc16' },
  { name: 'Orange', hex: '#f97316' },
  { name: 'Slate', hex: '#64748b' },
]

/** The colour number a layer has: the chosen one, or one that follows from its place. The base layer has none unless chosen. */
export function layerColorNumber(colors: number[], layer: number): number {
  const chosen = colors[layer] ?? 0
  if (chosen >= 1 && chosen <= LAYER_COLORS.length) return chosen
  return layer === 0 ? 0 : ((layer - 1) % LAYER_COLORS.length) + 1
}

export function layerHex(colors: number[], layer: number): string | undefined {
  return LAYER_COLORS[layerColorNumber(colors, layer) - 1]?.hex
}

export const layerColor = (session: Session, layer: number) => layerHex(session.layerColors, layer)

/**
 * The layers that are on when `layer` is the highest one on, highest first:
 * itself, the layers that turn it on together (and what turns those on), and
 * the base. A see-through key falls to the first of them that has a key.
 */
export function onWith(layersWith: number[], layer: number): number[] {
  let mask = 1 | (1 << layer)
  for (let before = 0; before !== mask; ) {
    before = mask
    for (let at = 1; at <= layer; at++) if ((mask >> at) & 1) mask |= layersWith[at] ?? 0
  }
  return [...Array(layer + 1).keys()].reverse().filter((at) => (mask >> at) & 1)
}

// --- Drawing a layer -----------------------------------------------------------

/** What a key does on `layer`, and the layer that comes from: a see-through key does what the first layer under it with a key says. */
export function effective(session: Pick<Session, 'layers' | 'layersWith'>, layer: number, pos: number): { action: Action; layer: number } | undefined {
  const own = session.layers[layer]?.[pos]
  if (own === undefined) return undefined
  if (own !== 'Transparent') return { action: own, layer }
  for (const below of onWith(session.layersWith, layer).slice(1)) {
    const lower = session.layers[below]?.[pos]
    if (lower !== undefined && lower !== 'Transparent') return { action: lower, layer: below }
  }
  return { action: own, layer: 0 }
}

/** What to print on the key at `pos` when `layer` is drawn. A see-through key shows, faintly, what it falls through to; a key that plays a named macro shows the name, as far as it fits. */
export function layerLegend(session: Pick<Session, 'layers' | 'layersWith' | 'macroNames'>, layer: number, pos: number): Legend | undefined {
  const found = effective(session, layer, pos)
  if (!found) return undefined
  if (found.layer !== layer && found.action !== 'Transparent') return { ...legend(found.action), kind: 'through' }
  const { action } = found
  const drawn = legend(action)
  const basic = typeof action === 'object' && 'Basic' in action ? action.Basic : undefined
  const index = typeof basic === 'object' && 'Macro' in basic ? basic.Macro : undefined
  const name = index === undefined ? '' : (session.macroNames[index] ?? '')
  if (!name) return drawn
  return { ...drawn, main: name.length > 7 ? name.slice(0, 6) + '…' : name, sub: `M${index! + 1}` }
}

/**
 * The held keys, apart: those the keyboard is doing what `layer` draws on
 * them, and those that used another layer or that the keyboard has not acted
 * on yet (an undecided hold-tap, part of a combo).
 */
export function heldOn(session: Pick<Session, 'layers' | 'layersWith' | 'pressed' | 'usedLayer' | 'layerAware'>, layer: number): { here: Set<number>; elsewhere: Set<number> } {
  const here = new Set<number>()
  const elsewhere = new Set<number>()
  for (const pos of session.pressed) {
    const exact = !session.layerAware || session.usedLayer.get(pos) === (effective(session, layer, pos)?.layer ?? layer)
    ;(exact ? here : elsewhere).add(pos)
  }
  return { here, elsewhere }
}

/** The layer new presses are looked up on first; none on firmware that does not say. */
export const topLayer = (session: Pick<Session, 'layerAware' | 'activeLayers'>) => (session.layerAware ? 31 - Math.clz32(session.activeLayers | 1) : undefined)

const tapOf = (action: Action): Basic | undefined => (action === 'Transparent' ? undefined : 'Basic' in action ? action.Basic : action.HoldTap.tap)

/** How well a key types what `usage` with `mods` held types: 2 exactly, 1 with the modifiers left to other keys, 0 not at all. */
function types(action: Action, usage: number, mods: number): number {
  const basic = tapOf(action)
  if (basic === undefined || basic === 'None') return 0
  if ('Key' in basic) return basic.Key === usage ? (mods === 0 ? 2 : 1) : 0
  if ('ModKey' in basic) return basic.ModKey.key === usage && basic.ModKey.mods === mods && mods !== 0 ? 2 : 0
  return 0
}

/**
 * Where the key is that types `usage` (with `mods` held): on `layer` as it is
 * drawn, else on the lowest other layer that has it.
 */
export function whereIs(session: Pick<Session, 'layers' | 'layersWith'>, layer: number, usage: number, mods = 0): { layer: number; pos: number } | undefined {
  const best = (score: (pos: number) => number, count: number) => {
    let at = -1
    let top = 0
    for (let pos = 0; pos < count; pos++) if (score(pos) > top) [at, top] = [pos, score(pos)]
    return at
  }
  const count = session.layers[layer]?.length ?? 0
  const here = best((pos) => types(effective(session, layer, pos)?.action ?? 'Transparent', usage, mods), count)
  if (here >= 0) return { layer, pos: here }
  for (const [other, actions] of session.layers.entries()) {
    if (other === layer) continue
    const pos = best((at) => types(actions[at], usage, mods), actions.length)
    if (pos >= 0) return { layer: other, pos }
  }
  return undefined
}
