// A colour per layer, so a layer and the keys that lead to it are found at a glance.

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
