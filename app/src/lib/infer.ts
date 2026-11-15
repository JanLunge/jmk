// Works out how a keyboard is wired from what the firmware's probe saw while
// the user pressed each key once.

import type { Contact, DiodeDirection, MatrixConfig, Pin } from './protocol'

/** What pressing one key connected. */
export type KeySignature =
  | { kind: 'ground'; pin: Pin }
  | { kind: 'supply'; pin: Pin }
  /** `diode`: current only flows one way, from `sensed` to `driven`. */
  | { kind: 'link'; driven: Pin; sensed: Pin; diode: boolean }

export function contactKey(contact: Contact): string {
  if ('Ground' in contact) return `g${contact.Ground}`
  if ('Supply' in contact) return `s${contact.Supply}`
  return `l${contact.Link.driven}>${contact.Link.sensed}`
}

/**
 * The key that a set of newly appeared contacts stands for, or undefined if
 * they do not look like exactly one key (e.g. two keys pressed together).
 */
export function signatureOf(contacts: Contact[]): KeySignature | undefined {
  if (contacts.length === 1) {
    const [contact] = contacts
    if ('Ground' in contact) return { kind: 'ground', pin: contact.Ground }
    if ('Supply' in contact) return { kind: 'supply', pin: contact.Supply }
    return { kind: 'link', ...contact.Link, diode: true }
  }
  if (contacts.length === 2) {
    const [a, b] = contacts
    if ('Link' in a && 'Link' in b && a.Link.driven === b.Link.sensed && a.Link.sensed === b.Link.driven) {
      // Seen in both directions: a switch without a diode.
      const [driven, sensed] = [a.Link.driven, a.Link.sensed].sort((x, y) => x - y)
      return { kind: 'link', driven, sensed, diode: false }
    }
  }
  return undefined
}

export function sameKey(a: KeySignature, b: KeySignature): boolean {
  if (a.kind !== b.kind) return false
  if (a.kind === 'link' && b.kind === 'link') {
    const direct = a.driven === b.driven && a.sensed === b.sensed
    return direct || (!a.diode && !b.diode && a.driven === b.sensed && a.sensed === b.driven)
  }
  return (a as { pin: Pin }).pin === (b as { pin: Pin }).pin
}

export interface Wiring {
  matrix: MatrixConfig
  /** For each pressed key, in press order, its position in the keymap. */
  positions: number[]
  /** Things the user should know, in plain words. */
  notes: string[]
}

export type InferResult = { ok: true; wiring: Wiring } | { ok: false; problem: string }

export function inferWiring(keys: KeySignature[]): InferResult {
  if (keys.length === 0) return { ok: false, problem: 'No keys were pressed yet.' }
  const direct = keys.filter((key) => key.kind !== 'link')
  const links = keys.filter((key) => key.kind === 'link')
  if (direct.length && links.length) {
    return {
      ok: false,
      problem:
        'Some keys go straight to ground and others are in a matrix. JMK needs one wiring style per board for now.',
    }
  }
  if (direct.length) {
    const activeHigh = direct[0].kind === 'supply'
    if (direct.some((key) => (key.kind === 'supply') !== activeHigh)) {
      return { ok: false, problem: 'Some keys connect to ground and others to supply voltage. They must all be the same.' }
    }
    return {
      ok: true,
      wiring: {
        matrix: { Direct: { pins: direct.map((key) => (key as { pin: Pin }).pin), active_high: activeHigh } },
        positions: direct.map((_, i) => i),
        notes: [
          activeHigh
            ? 'Each key connects its own pin to supply voltage.'
            : 'Each key connects its own pin to ground. Simple and reliable; it uses one pin per key.',
        ],
      },
    }
  }
  return inferMatrix(links)
}

function inferMatrix(keys: Extract<KeySignature, { kind: 'link' }>[]): InferResult {
  const withDiodes = keys.filter((key) => key.diode)
  if (withDiodes.length && withDiodes.length !== keys.length) {
    return { ok: false, problem: 'Some keys have a diode and others do not. Check the diodes on the keys without one.' }
  }
  // Split the pins into two groups so that every key joins one pin of each.
  const side = new Map<Pin, 0 | 1>()
  if (withDiodes.length) {
    for (const key of keys) {
      if (side.get(key.driven) === 1 || side.get(key.sensed) === 0) {
        return { ok: false, problem: 'The diodes do not all point the same way. One of them is probably soldered backwards.' }
      }
      side.set(key.driven, 0)
      side.set(key.sensed, 1)
    }
  } else {
    // Without diodes the direction is unknown: give neighbouring pins
    // opposite sides until every key is placed or two pins clash.
    const pending = new Set(keys)
    while (pending.size) {
      let progressed = false
      for (const key of pending) {
        const a = side.get(key.driven)
        const b = side.get(key.sensed)
        if (a === undefined && b === undefined) continue
        if (a !== undefined && b !== undefined && a === b) {
          return {
            ok: false,
            problem: 'The wiring does not form rows and columns. Check for a wire joining two rows or two columns.',
          }
        }
        if (a === undefined) side.set(key.driven, (1 - b!) as 0 | 1)
        if (b === undefined) side.set(key.sensed, (1 - a!) as 0 | 1)
        pending.delete(key)
        progressed = true
      }
      if (!progressed) {
        // A part of the matrix not yet connected to the rest: start it off.
        const [key] = pending
        side.set(key.driven, 0)
      }
    }
  }
  const group = (which: 0 | 1) => [...side].filter(([, s]) => s === which).map(([pin]) => pin)
  const drivenPins = group(0)
  const sensedPins = group(1)
  // Call the smaller group rows; it reads more naturally for most boards.
  const drivenAreRows = drivenPins.length <= sensedPins.length
  const rows = drivenAreRows ? drivenPins : sensedPins
  const cols = drivenAreRows ? sensedPins : drivenPins
  // The firmware drives rows for Col2Row and columns for Row2Col.
  const diodes: DiodeDirection = drivenAreRows ? 'Col2Row' : 'Row2Col'
  const positions = keys.map((key) => {
    const pinOnSide = (which: 0 | 1) => (side.get(key.driven) === which ? key.driven : key.sensed)
    const row = rows.indexOf(pinOnSide(drivenAreRows ? 0 : 1))
    const col = cols.indexOf(pinOnSide(drivenAreRows ? 1 : 0))
    return row * cols.length + col
  })
  const notes = [`A matrix of ${rows.length} row${rows.length === 1 ? '' : 's'} and ${cols.length} column${cols.length === 1 ? '' : 's'}.`]
  if (withDiodes.length) notes.push('Every key has a diode, so any combination of keys can be held at once.')
  else notes.push('The keys have no diodes. Holding three keys that form a corner can make a fourth appear pressed.')
  return { ok: true, wiring: { matrix: { Matrix: { rows, cols, diodes } }, positions, notes } }
}
