// Fits one more key into wiring that is already set up, for a switch that
// was missed during setup or soldered in later.

import type { KeySignature } from './infer'
import { keyCount, wiringParts } from './definition'
import { LIMITS, NO_SCAN, type HardwareConfig, type MatrixConfig, type Pin } from './protocol'

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

/**
/**
 * Work out how the key that made `key` joins the wiring in `hardware`, on a
 * keyboard that can have `maxMore` groups of switches besides its first. Keys
 * read the other way round keep that, wherever the new numbering puts them.
 */
export function planAddedKey(hardware: HardwareConfig, key: KeySignature, maxMore: number = LIMITS.moreWiring): AddPlan {
  const plan = planDoubles(hardware, key, maxMore)
  if (!plan.ok) return plan
  const before = hardware.scan?.inverted ?? []
  const inverted = [...new Set([...before.map(plan.move), ...(key.inverted ? [plan.pos] : [])])].sort((a, b) => a - b)
  if (inverted.length === before.length && inverted.every((pos, i) => pos === before[i])) return plan
  return { ...plan, hardware: { ...plan.hardware, scan: { ...(hardware.scan ?? NO_SCAN), inverted } } }
}

function planDoubles(hardware: HardwareConfig, key: KeySignature, maxMore: number): AddPlan {
  const plan = planGroups(hardware, key, maxMore)
  if (!plan.ok) return plan
  const { double, ...rest } = plan
  // Positions that read a key a second time are renumbered like any other.
  const doubles = (hardware.doubles ?? []).map(plan.move)
  const added = double !== undefined && !doubles.includes(double)
  if (added) doubles.push(double)
  if (doubles.length > LIMITS.doubles) {
    return { ok: false, problem: 'This keyboard already has as many keys without a diode among ones with diodes both ways as it can hold.' }
  }
  if (!doubles.length) return rest
  return { ...rest, hardware: { ...plan.hardware, doubles }, rewired: plan.rewired || added }
}

/** `double`: a position that reads the new key a second time and must not be reported. */
function planGroups(hardware: HardwareConfig, key: KeySignature, maxMore: number): AddPlan & { double?: number } {
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
      if (parts.length > maxMore) return { ok: false, problem: 'This keyboard already mixes as many wiring styles as it can hold.' }
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

  const isMatrix = (part: MatrixConfig): part is Extract<MatrixConfig, { Matrix: unknown }> => part !== 'None' && 'Matrix' in part
  /** The matrices the key's pins belong to already. */
  const owners = parts.flatMap((part, i) => (isMatrix(part) && [...part.Matrix.rows, ...part.Matrix.cols].some((pin) => pin === key.driven || pin === key.sensed) ? [i] : []))
  if (owners.length > 1) {
    return { ok: false, problem: 'This key joins two groups of keys that are read apart from each other. Find the keys again to set the wiring up anew.' }
  }
  let index = owners[0] ?? -1
  if (index < 0 && parts.length <= maxMore) {
    // Two pins no other key uses: a group of its own after the others.
    return {
      ok: true,
      hardware: { ...hardware, more: [...(hardware.more ?? []), { Matrix: { rows: [key.driven], cols: [key.sensed], diodes: 'Col2Row' } }] },
      pos: switches,
      move: (old) => (old < switches ? old : old + 1),
      rewired: true,
    }
  }
  // No room for another group: the first matrix takes it in, as a new row and column.
  if (index < 0) index = parts.findIndex(isMatrix)
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
    // Read in both directions, a key without a diode also shows as its crossing's second key.
    double: duplex && !key.diode ? pos + newCrossings : undefined,
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
