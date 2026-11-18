// Fits one more key into wiring that is already set up, for a switch that
// was missed during setup or soldered in later.

import type { KeySignature } from './infer'
import type { HardwareConfig, Pin } from './protocol'

export type AddPlan =
  | {
      ok: true
      hardware: HardwareConfig
      /** The key's position in the keymap, in the new numbering. */
      pos: number
      /** Where a position of the old numbering ends up. */
      move: (old: number) => number
      /** The wiring itself grew (a new pin, row or column). */
      rewired: boolean
    }
  | { ok: false; problem: string }

/** Work out how the key that made `key` joins the wiring in `hardware`. */
export function planAddedKey(hardware: HardwareConfig, key: KeySignature): AddPlan {
  const matrix = hardware.matrix
  if (matrix === 'None') return { ok: false, problem: 'This keyboard has no keys set up yet. Run the setup first.' }

  if ('Direct' in matrix) {
    const { pins, active_high } = matrix.Direct
    if (key.kind === 'link') {
      return { ok: false, problem: 'This key joins two pins, but the other keys each use one pin. Find the keys again to change the wiring style.' }
    }
    if ((key.kind === 'supply') !== active_high) {
      return { ok: false, problem: `This key connects to ${key.kind === 'supply' ? 'supply voltage' : 'ground'}, unlike the other keys.` }
    }
    const known = pins.indexOf(key.pin)
    if (known >= 0) return { ok: true, hardware, pos: known, move: (old) => old, rewired: false }
    const count = pins.length
    return {
      ok: true,
      hardware: { ...hardware, matrix: { Direct: { pins: [...pins, key.pin], active_high } } },
      pos: count,
      // Knob directions and the other half's keys come after this half's switches.
      move: (old) => (old < count ? old : old + 1),
      rewired: true,
    }
  }

  if (key.kind !== 'link') {
    return { ok: false, problem: 'This key connects one pin to ground or supply, but the other keys are in a matrix.' }
  }
  const { rows, cols, diodes } = matrix.Matrix
  const rowsDriven = diodes === 'Col2Row'
  const driven = rowsDriven ? rows : cols
  const sensed = rowsDriven ? cols : rows
  const clashes = (d: Pin, s: Pin) => sensed.includes(d) || driven.includes(s)
  let { driven: d, sensed: s } = key
  if (clashes(d, s)) {
    if (key.diode) return { ok: false, problem: "This key's diode points the other way round than the others. It is probably soldered backwards." }
    // Without a diode the direction means nothing; try it the other way.
    ;[d, s] = [s, d]
    if (clashes(d, s)) return { ok: false, problem: 'This key joins two rows or two columns. Check its wires.' }
  }
  const grown = (pins: Pin[], pin: Pin) => (pins.includes(pin) ? pins : [...pins, pin])
  const newRows = grown(rows, rowsDriven ? d : s)
  const newCols = grown(cols, rowsDriven ? s : d)
  const pos = newRows.indexOf(rowsDriven ? d : s) * newCols.length + newCols.indexOf(rowsDriven ? s : d)
  const rewired = newRows !== rows || newCols !== cols
  const oldCount = rows.length * cols.length
  const newCount = newRows.length * newCols.length
  return {
    ok: true,
    hardware: rewired ? { ...hardware, matrix: { Matrix: { rows: newRows, cols: newCols, diodes } } } : hardware,
    pos,
    move: (old) =>
      old < oldCount ? Math.floor(old / cols.length) * newCols.length + (old % cols.length) : old - oldCount + newCount,
    rewired,
  }
}
