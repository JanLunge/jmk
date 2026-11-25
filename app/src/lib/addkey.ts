// Fits one more key into wiring that is already set up, for a switch that
// was missed during setup or soldered in later.

import type { KeySignature } from './infer'
import { keyCount, wiringParts } from './definition'
import type { HardwareConfig, MatrixConfig, Pin } from './protocol'

/** Groups of switches a board can have besides its first (the firmware's limit). */
const MAX_MORE_WIRING = 3

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
  if (hardware.matrix === 'None') return { ok: false, problem: 'This keyboard has no keys set up yet. Run the setup first.' }
  const parts = wiringParts(hardware)
  /** Put a changed group back where it was; the first group lives in `matrix`. */
  const withPart = (index: number, part: MatrixConfig): HardwareConfig => {
    const next = parts.map((old, i) => (i === index ? part : old))
    return { ...hardware, matrix: next[0], more: next.length > 1 ? next.slice(1) : undefined }
  }
  const baseOf = (index: number) => parts.slice(0, index).reduce((sum, part) => sum + keyCount(part), 0)
  const switches = baseOf(parts.length)

  if (key.kind !== 'link') {
    const activeHigh = key.kind === 'supply'
    const index = parts.findIndex((part) => part !== 'None' && 'Direct' in part && part.Direct.active_high === activeHigh)
    if (index < 0) {
      // The first key of its kind: a group of its own after the others.
      if (parts.length > MAX_MORE_WIRING) return { ok: false, problem: 'This keyboard already mixes as many wiring styles as it can hold.' }
      return {
        ok: true,
        hardware: { ...hardware, more: [...(hardware.more ?? []), { Direct: { pins: [key.pin], active_high: activeHigh } }] },
        pos: switches,
        // Knob directions and the other half's keys come after this half's switches.
        move: (old) => (old < switches ? old : old + 1),
        rewired: true,
      }
    }
    const part = parts[index] as Extract<MatrixConfig, { Direct: unknown }>
    const { pins } = part.Direct
    const base = baseOf(index)
    const known = pins.indexOf(key.pin)
    if (known >= 0) return { ok: true, hardware, pos: base + known, move: (old) => old, rewired: false }
    const end = base + pins.length
    return {
      ok: true,
      hardware: withPart(index, { Direct: { pins: [...pins, key.pin], active_high: activeHigh } }),
      pos: end,
      move: (old) => (old < end ? old : old + 1),
      rewired: true,
    }
  }

  const index = parts.findIndex((part) => part !== 'None' && 'Matrix' in part)
  if (index < 0) {
    return { ok: false, problem: 'This key joins two pins, but the other keys each use one pin. Find the keys again to change the wiring style.' }
  }
  const { rows, cols, diodes } = (parts[index] as Extract<MatrixConfig, { Matrix: unknown }>).Matrix
  const base = baseOf(index)
  const duplex = diodes === 'Both'
  // Which of the key's two pins is a row. Rows are pulled low for Col2Row.
  const rowsDriven = diodes !== 'Row2Col'
  let { driven: d, sensed: s } = key
  /** With `d` on the side that is pulled low first and `s` on the other: does that fit? */
  const fits = (low: Pin, read: Pin) => {
    const [lowSide, readSide] = rowsDriven ? [rows, cols] : [cols, rows]
    return !readSide.includes(low) && !lowSide.includes(read)
  }
  let second = false
  if (!fits(d, s)) {
    if (!fits(s, d)) return { ok: false, problem: 'This key joins two rows or two columns. Check its wires.' }
    if (key.diode && !duplex) {
      return { ok: false, problem: "This key's diode points the other way round than the others. It is probably soldered backwards." }
    }
    // Without a diode the direction means nothing; in a duplex matrix it is the crossing's second key.
    ;[d, s] = [s, d]
    second = key.diode
  }
  const [rowPin, colPin] = rowsDriven ? [d, s] : [s, d]
  const grown = (pins: Pin[], pin: Pin) => (pins.includes(pin) ? pins : [...pins, pin])
  const newRows = grown(rows, rowPin)
  const newCols = grown(cols, colPin)
  const rewired = newRows !== rows || newCols !== cols
  const oldCrossings = rows.length * cols.length
  const newCrossings = newRows.length * newCols.length
  const blocks = duplex ? 2 : 1
  const pos = base + (second ? newCrossings : 0) + newRows.indexOf(rowPin) * newCols.length + newCols.indexOf(colPin)
  return {
    ok: true,
    hardware: rewired ? withPart(index, { Matrix: { rows: newRows, cols: newCols, diodes } }) : hardware,
    pos,
    move: (old) => {
      if (old < base) return old
      const local = old - base
      if (local >= oldCrossings * blocks) return old - oldCrossings * blocks + newCrossings * blocks
      const block = Math.floor(local / oldCrossings)
      const crossing = local % oldCrossings
      return base + block * newCrossings + Math.floor(crossing / cols.length) * newCols.length + (crossing % cols.length)
    },
    rewired,
  }
}
