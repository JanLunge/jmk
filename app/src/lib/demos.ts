// Virtual keyboards for trying the configurator without hardware. Each one
// is "soldered" a different way and starts unconfigured, exactly like a
// freshly flashed board.

import type { KeyGeometry, Pin, Wire, Wiring } from './protocol'
import { UNIT } from './layout'

export interface DemoHalf {
  label: string
  wiring: Wiring
  /** Where each switch physically sits, for the clickable picture. */
  keys: KeyGeometry[]
  /** Pins of the serial cable to the other half, if this is a split. */
  link?: { tx: Pin; rx: Pin }
  /** A chain of RGB lights soldered to a pin. */
  lights?: { pin: Pin; count: number }
}

export interface Demo {
  id: string
  name: string
  blurb: string
  halves: DemoHalf[]
}

const unit = (col: number, row: number): KeyGeometry => ({ x: col * UNIT, y: row * UNIT, w: UNIT, h: UNIT, r: 0 })

/** A matrix with diodes from column to row, switches in reading order; `skip` lists empty spots. */
function matrix(rows: Pin[], cols: Pin[], skip: [number, number][] = []): { wires: Wire[]; keys: KeyGeometry[] } {
  const wires: Wire[] = []
  const keys: KeyGeometry[] = []
  rows.forEach((row, r) =>
    cols.forEach((col, c) => {
      if (skip.some(([sr, sc]) => sr === r && sc === c)) return
      wires.push({ Diode: { anode: col, cathode: row } })
      keys.push(unit(c, r))
    }),
  )
  return { wires, keys }
}

function splitHalf(label: string, mirrored: boolean): DemoHalf {
  // Three rows of five plus two thumb keys, wired as a 4x5 matrix.
  const grid = matrix([2, 3, 4, 5], [6, 7, 8, 9, 10], [[3, 0], [3, 1], [3, 2]])
  const keys = grid.keys.map((key, index) => {
    const col = key.x / UNIT
    const row = key.y / UNIT
    const x = mirrored ? 4 - col : col
    const stagger = [20, 10, 0, 10, 25][col]
    if (index >= 15) return { ...unit(x, 3), x: x * UNIT + (mirrored ? 30 : -30), y: 3 * UNIT + 45, r: mirrored ? -120 : 120 }
    return { ...unit(x, row), y: row * UNIT + stagger }
  })
  return { label, wiring: { switches: grid.wires, encoders: [] }, keys, link: { tx: 0, rx: 1 } }
}

export const DEMOS: Demo[] = [
  {
    id: 'macropad',
    name: 'Macro pad',
    blurb: 'Six keys in a small diode matrix. The quickest way to see the whole setup.',
    halves: [(() => {
      const grid = matrix([16, 17], [18, 19, 20])
      return { label: 'Macro pad', wiring: { switches: grid.wires, encoders: [] }, keys: grid.keys }
    })()],
  },
  {
    id: 'knobpad',
    name: 'Pad with a knob',
    blurb: 'Four keys wired straight to pins, a rotary encoder and six RGB lights.',
    halves: [{
      label: 'Knob pad',
      wiring: { switches: [2, 3, 4, 5].map((pin) => ({ Ground: pin })), encoders: [[10, 11]] },
      lights: { pin: 16, count: 6 },
      keys: [unit(0, 1), unit(1, 1), unit(2, 1), unit(3, 1)],
    }],
  },
  {
    id: 'forty',
    name: 'Hand-wired 40%',
    blurb: 'Forty keys in a four by ten matrix, like a typical first hand-wired build.',
    halves: [(() => {
      const grid = matrix([2, 3, 4, 5], [6, 7, 8, 9, 10, 11, 12, 13, 14, 15])
      return { label: '40% keyboard', wiring: { switches: grid.wires, encoders: [] }, keys: grid.keys }
    })()],
  },
  {
    id: 'duplex',
    name: 'Duplex pad',
    blurb: 'Twelve keys on five pins in a duplex matrix, two per crossing, plus three keys on pins of their own.',
    halves: [(() => {
      const wires: Wire[] = []
      const keys: KeyGeometry[] = []
      ;[2, 3].forEach((row, r) =>
        [4, 5, 6].forEach((col, c) => {
          // The two switches of a crossing sit side by side, their diodes pointing opposite ways.
          wires.push({ Diode: { anode: col, cathode: row } }, { Diode: { anode: row, cathode: col } })
          keys.push(unit(2 * c, r), unit(2 * c + 1, r))
        }),
      )
      wires.push({ Ground: 10 }, { Ground: 11 }, { Supply: 12 })
      keys.push(unit(1.5, 2.25), unit(2.5, 2.25), unit(3.5, 2.25))
      return { label: 'Duplex pad', wiring: { switches: wires, encoders: [] }, keys }
    })()],
  },
  {
    id: 'split',
    name: 'Split keyboard',
    blurb: 'Two halves of seventeen keys joined by a cable. Each half has its own controller. Split support is experimental.',
    halves: [splitHalf('Left half', false), splitHalf('Right half', true)],
  },
]
