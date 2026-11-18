// A keyboard written down as a file: what the registry stores and what
// "export" produces. Applying one to a keyboard configures it completely.

import type { Action, Combo, HardwareConfig, HoldTapProfile, KeyGeometry, LightingConfig, MacroStep, Mcu } from './protocol'

export const DEFINITION_SCHEMA = 1

export interface Keymap {
  layers: Action[][]
  profiles: HoldTapProfile[]
  combos: Combo[]
  macros: MacroStep[][]
  /** A colour number per layer (see layers.ts). */
  layerColors?: number[]
}

export interface Definition {
  schema: number
  name: string
  controller: string
  mcu: Mcu
  /** One entry for a keyboard in one piece; central half first for a split. */
  halves: { hardware: HardwareConfig }[]
  /** One shape per key position, across both halves. */
  layout: KeyGeometry[]
  keymap: Keymap
  /** RGB lights, if the keyboard has them. */
  lighting?: LightingConfig
}

export function parseDefinition(text: string): Definition {
  let data: unknown
  try {
    data = JSON.parse(text)
  } catch {
    throw new Error('This file is not valid JSON.')
  }
  const d = data as Partial<Definition>
  if (!d || typeof d !== 'object' || typeof d.schema !== 'number') throw new Error('This is not a JMK keyboard file.')
  if (d.schema > DEFINITION_SCHEMA) throw new Error('This file was made by a newer version of the configurator.')
  if (!Array.isArray(d.halves) || !d.halves.length || !Array.isArray(d.layout) || !d.keymap || !Array.isArray(d.keymap.layers)) {
    throw new Error('This keyboard file is incomplete.')
  }
  return d as Definition
}

/** Number of keymap positions a definition's hardware produces. */
export function positionCount(halves: { hardware: HardwareConfig }[]): number {
  return halves.reduce((sum, half) => sum + localPositions(half.hardware), 0)
}

export function localPositions(hardware: HardwareConfig): number {
  const matrix = hardware.matrix
  const keys = matrix === 'None' ? 0 : 'Direct' in matrix ? matrix.Direct.pins.length : matrix.Matrix.rows.length * matrix.Matrix.cols.length
  return keys + 2 * hardware.encoders.length
}
