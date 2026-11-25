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
  /** Further groups of switches, for a board that mixes wiring styles. */
  more: MatrixConfig[]
  /** For each pressed key, in press order, its position in the keymap. */
  positions: number[]
  /** Things the user should know, in plain words. */
  notes: string[]
}

export type InferResult = { ok: true; wiring: Wiring } | { ok: false; problem: string }

/** One group of switches and, for each of its keys in press order, the position within the group. */
interface Part {
  config: MatrixConfig
  /** Index into the pressed keys, and where that key sits in this group. */
  keys: { key: number; local: number }[]
  size: number
  notes: string[]
}

/** The most switch positions one board can have (the firmware's limit). */
const MAX_POSITIONS = 128

/**
 * A board may mix styles: a matrix, keys that each connect a pin of their
 * own to ground, and keys that connect one to supply voltage. Each style is
 * a group, and the groups take their positions one after the other, the
 * matrix first.
 */
export function inferWiring(keys: KeySignature[]): InferResult {
  if (keys.length === 0) return { ok: false, problem: 'No keys were pressed yet.' }
  const indexed = keys.map((key, index) => ({ key, index }))
  const parts: Part[] = []

  const links = indexed.filter(({ key }) => key.kind === 'link')
  if (links.length) {
    const matrix = inferMatrix(links.map(({ key }) => key as Extract<KeySignature, { kind: 'link' }>))
    if ('problem' in matrix) return { ok: false, problem: matrix.problem }
    parts.push({ ...matrix, keys: links.map(({ index }, i) => ({ key: index, local: matrix.positions[i] })) })
  }
  for (const kind of ['ground', 'supply'] as const) {
    const group = indexed.filter(({ key }) => key.kind === kind)
    if (!group.length) continue
    const activeHigh = kind === 'supply'
    parts.push({
      config: { Direct: { pins: group.map(({ key }) => (key as { pin: Pin }).pin), active_high: activeHigh } },
      keys: group.map(({ index }, local) => ({ key: index, local })),
      size: group.length,
      notes: [
        activeHigh
          ? `${count(group.length)} its own pin to supply voltage.`
          : `${count(group.length)} its own pin to ground. Simple and reliable; it uses one pin per key.`,
      ],
    })
  }

  const positions: number[] = Array(keys.length).fill(0)
  let base = 0
  for (const part of parts) {
    for (const { key, local } of part.keys) positions[key] = base + local
    base += part.size
  }
  if (base > MAX_POSITIONS) return { ok: false, problem: `This wiring has room for ${base} keys, but a board can have ${MAX_POSITIONS}.` }
  const notes = parts.flatMap((part) => part.notes)
  if (parts.length > 1) notes.push('The keyboard mixes these wiring styles; each group is read in its own way.')
  return { ok: true, wiring: { matrix: parts[0].config, more: parts.slice(1).map((part) => part.config), positions, notes } }
}

const count = (keys: number) => (keys === 1 ? 'One key connects' : `${keys} keys each connect`)

function inferMatrix(
  keys: Extract<KeySignature, { kind: 'link' }>[],
): { config: MatrixConfig; positions: number[]; size: number; notes: string[] } | { problem: string } {
  const withDiodes = keys.filter((key) => key.diode)
  if (withDiodes.length && withDiodes.length !== keys.length) {
    return { problem: 'Some keys have a diode and others do not. Check the diodes on the keys without one.' }
  }
  // Split the pins into two groups so that every key joins one pin of each:
  // give neighbouring pins opposite sides until every key is placed or two
  // pins clash. A part of the matrix not yet joined to the rest starts with
  // the pin that was pulled low on side 0, which is where every such pin
  // ends up when all diodes point the same way.
  const side = new Map<Pin, 0 | 1>()
  const pending = new Set(keys)
  while (pending.size) {
    let progressed = false
    for (const key of pending) {
      const a = side.get(key.driven)
      const b = side.get(key.sensed)
      if (a === undefined && b === undefined) continue
      if (a !== undefined && b !== undefined && a === b) {
        return { problem: 'The wiring does not form rows and columns. Check for a wire joining two rows or two columns.' }
      }
      if (a === undefined) side.set(key.driven, (1 - b!) as 0 | 1)
      if (b === undefined) side.set(key.sensed, (1 - a!) as 0 | 1)
      pending.delete(key)
      progressed = true
    }
    if (!progressed) {
      const [key] = pending
      side.set(key.driven, 0)
    }
  }
  const group = (which: 0 | 1) => [...side].filter(([, s]) => s === which).map(([pin]) => pin)
  const drivenPins = group(0)
  const sensedPins = group(1)
  // Call the smaller group rows; it reads more naturally for most boards.
  const drivenAreRows = drivenPins.length <= sensedPins.length
  const rows = drivenAreRows ? drivenPins : sensedPins
  const cols = drivenAreRows ? sensedPins : drivenPins
  // With diodes, a key whose low pin is on side 1 conducts the other way.
  const backwards = (key: (typeof keys)[number]) => key.diode && side.get(key.driven) === 1
  const duplex = keys.some(backwards)
  // The firmware drives rows for Col2Row and columns for Row2Col.
  const diodes: DiodeDirection = duplex ? 'Both' : drivenAreRows ? 'Col2Row' : 'Row2Col'
  const crossings = rows.length * cols.length
  const positions = keys.map((key) => {
    const pinOnSide = (which: 0 | 1) => (side.get(key.driven) === which ? key.driven : key.sensed)
    const row = rows.indexOf(pinOnSide(drivenAreRows ? 0 : 1))
    const col = cols.indexOf(pinOnSide(drivenAreRows ? 1 : 0))
    // In a duplex matrix the keys found by pulling a row low come first.
    const second = duplex && !rows.includes(key.driven)
    return (second ? crossings : 0) + row * cols.length + col
  })
  if (new Set(positions).size !== positions.length) {
    return { problem: 'Two keys join the same two pins the same way round. Check for keys wired in parallel.' }
  }
  const notes = [`A matrix of ${rows.length} row${rows.length === 1 ? '' : 's'} and ${cols.length} column${cols.length === 1 ? '' : 's'}.`]
  if (duplex) {
    const few = keys.filter((key) => rows.includes(key.driven) !== drivenAreRows).length
    notes.push(
      `The diodes point both ways (${few} of ${keys.length} keys the other way round), so the matrix is read in both directions. ` +
        'That is right for a duplex matrix with two keys per crossing; if yours is not one, those diodes are soldered backwards. ' +
        'Holding four or more keys at once can then make another appear pressed.',
    )
  } else if (withDiodes.length) notes.push('Every key has a diode, so any combination of keys can be held at once.')
  else notes.push('The keys have no diodes. Holding three keys that form a corner can make a fourth appear pressed.')
  return { config: { Matrix: { rows, cols, diodes } }, positions, size: crossings * (duplex ? 2 : 1), notes }
}
