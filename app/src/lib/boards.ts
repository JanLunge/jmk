// Controller boards the configurator can draw. A board says which chip pins
// are reachable and where, so the user can point at real solder pads.

import type { Mcu, Pin } from './protocol'

export interface Pad {
  /** Printed name on the board. */
  label: string
  /** Chip pin number, or undefined for power, ground and reset pads. */
  pin?: Pin
  kind: 'gpio' | 'power' | 'ground' | 'other'
}

export interface Board {
  id: string
  name: string
  /** Other boards with the same pad layout. */
  alsoKnownAs?: string
  mcu: Mcu
  /** Pads down the left and right edge, USB connector at the top. */
  left: Pad[]
  right: Pad[]
  /** Pads along the bottom edge, left to right. */
  bottom?: Pad[]
  /** Chip pins wired to things on the board itself; never offered for keys. */
  internal: { pin: Pin; use: string }[]
  /** Width of the board in pad pitches, for drawing. */
  width: number
  /** The pad layout was written from memory and not checked against a board. */
  unverified?: boolean
}

const gp = (n: number): Pad => ({ label: `GP${n}`, pin: n, kind: 'gpio' })
const gnd: Pad = { label: 'GND', kind: 'ground' }
const pwr = (label: string): Pad => ({ label, kind: 'power' })
const other = (label: string): Pad => ({ label, kind: 'other' })
/** nRF52840 pads are named by port: P0.n is pin n, P1.n is pin 32 + n. */
const p = (port: 0 | 1, n: number): Pad => ({
  label: `P${port}.${String(n).padStart(2, '0')}`,
  pin: port * 32 + n,
  kind: 'gpio',
})

export const BOARDS: Board[] = [
  {
    id: 'pico',
    name: 'Raspberry Pi Pico',
    mcu: 'Rp2040',
    width: 8,
    left: [
      gp(0), gp(1), gnd, gp(2), gp(3), gp(4), gp(5), gnd, gp(6), gp(7),
      gp(8), gp(9), gnd, gp(10), gp(11), gp(12), gp(13), gnd, gp(14), gp(15),
    ],
    right: [
      pwr('VBUS'), pwr('VSYS'), gnd, other('3V3 EN'), pwr('3V3'), other('VREF'), gp(28), gnd, gp(27), gp(26),
      other('RUN'), gp(22), gnd, gp(21), gp(20), gp(19), gp(18), gnd, gp(17), gp(16),
    ],
    internal: [
      { pin: 23, use: 'power supply mode' },
      { pin: 24, use: 'USB power sense' },
      { pin: 25, use: 'on-board LED' },
      { pin: 29, use: 'supply voltage sense' },
    ],
  },
  {
    id: 'promicro-rp2040',
    name: 'Pro Micro style RP2040',
    alsoKnownAs: '0xCB Helios, Elite-Pi, Sea-Picro',
    mcu: 'Rp2040',
    width: 7,
    unverified: true,
    left: [gp(0), gp(1), gnd, gnd, gp(2), gp(3), gp(4), gp(5), gp(6), gp(7), gp(8), gp(9)],
    right: [pwr('RAW'), gnd, other('RST'), pwr('3V3'), gp(29), gp(28), gp(27), gp(26), gp(22), gp(20), gp(23), gp(21)],
    bottom: [gp(12), gp(13), gp(14), gp(15), gp(16)],
    internal: [{ pin: 17, use: 'on-board LED' }],
  },
  {
    id: 'nice-nano',
    name: 'nice!nano v2',
    alsoKnownAs: 'most Pro Micro style nRF52840 boards',
    mcu: 'Nrf52840',
    width: 7,
    unverified: true,
    left: [p(0, 6), p(0, 8), gnd, gnd, p(0, 17), p(0, 20), p(0, 22), p(0, 24), p(1, 0), p(0, 11), p(1, 4), p(1, 6)],
    right: [pwr('BAT+'), gnd, other('RST'), pwr('3V3'), p(0, 31), p(0, 29), p(0, 2), p(1, 15), p(1, 13), p(1, 11), p(0, 10), p(0, 9)],
    internal: [
      { pin: 13, use: 'power to external devices' },
      { pin: 15, use: 'on-board LED' },
      { pin: 4, use: 'battery voltage sense' },
    ],
  },
]

/** A board we know nothing about: every usable chip pin in two columns. */
export function genericBoard(mcu: Mcu, usablePins: number): Board {
  const pins: Pin[] = []
  for (let pin = 0; pin < 53; pin++) if (Math.floor(usablePins / 2 ** pin) % 2 === 1) pins.push(pin)
  const pads = pins.map((pin) => (mcu === 'Nrf52840' ? p(pin < 32 ? 0 : 1, pin % 32) : gp(pin)))
  const half = Math.ceil(pads.length / 2)
  return {
    id: 'generic',
    name: 'Other board',
    mcu,
    width: 8,
    left: pads.slice(0, half),
    right: pads.slice(half),
    internal: [],
  }
}

export function boardsFor(mcu: Mcu): Board[] {
  // Virtual keyboards behave like an RP2040.
  const family = mcu === 'Simulated' ? 'Rp2040' : mcu
  return BOARDS.filter((board) => board.mcu === family)
}

export function findBoard(id: string, mcu: Mcu, usablePins: number): Board {
  return BOARDS.find((board) => board.id === id) ?? genericBoard(mcu, usablePins)
}

export function boardPads(board: Board): Pad[] {
  return [...board.left, ...board.right, ...(board.bottom ?? [])]
}

/** Pins that can carry keyboard wiring on this board. */
export function wirablePins(board: Board, usablePins: number): Pin[] {
  const internal = new Set(board.internal.map((i) => i.pin))
  return boardPads(board)
    .filter((pad) => pad.pin !== undefined && !internal.has(pad.pin))
    .map((pad) => pad.pin!)
    .filter((pin) => Math.floor(usablePins / 2 ** pin) % 2 === 1)
}

export function pinLabel(board: Board | undefined, pin: Pin): string {
  const pad = board && boardPads(board).find((candidate) => candidate.pin === pin)
  return pad?.label ?? (board?.mcu === 'Nrf52840' ? `P${pin < 32 ? 0 : 1}.${String(pin % 32).padStart(2, '0')}` : `GP${pin}`)
}
