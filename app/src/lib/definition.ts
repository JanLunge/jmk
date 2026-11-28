// A keyboard written down as a file: what "Save as file" produces. Applying one to a keyboard configures it completely.

import type { Action, Combo, HardwareConfig, HoldTapProfile, KeyGeometry, LightingConfig, MacroStep, MatrixConfig, Mcu, Pin } from './protocol'

export const DEFINITION_SCHEMA = 1

export interface Keymap {
  layers: Action[][]
  profiles: HoldTapProfile[]
  combos: Combo[]
  macros: MacroStep[][]
  /** What the user calls each macro; empty for one without a name. */
  macroNames?: string[]
  /** A colour number per layer (see layers.ts). */
  layerColors?: number[]
  /** Per layer: the layers, one bit each, that turn it on while all of them are on. */
  layersWith?: number[]
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
  /** The key positions of the top, home and bottom row of letters, left to right. */
  letterRows?: number[][]
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

/** Switches in one group of a board's wiring. */
export function keyCount(part: MatrixConfig): number {
  if (part === 'None') return 0
  if ('Direct' in part) return part.Direct.pins.length
  const { rows, cols, diodes } = part.Matrix
  return rows.length * cols.length * (diodes === 'Both' ? 2 : 1)
}

/** Every group of switches on a board, in the order of their positions. */
export const wiringParts = (hardware: HardwareConfig): MatrixConfig[] => [hardware.matrix, ...(hardware.more ?? [])]

/** The pins one group of switches is on. */
export function partPins(part: MatrixConfig): Pin[] {
  if (part === 'None') return []
  return 'Direct' in part ? part.Direct.pins : [...part.Matrix.rows, ...part.Matrix.cols]
}

export const switchCount = (hardware: HardwareConfig) => wiringParts(hardware).reduce((sum, part) => sum + keyCount(part), 0)

export function localPositions(hardware: HardwareConfig): number {
  return switchCount(hardware) + 2 * hardware.encoders.length
}
