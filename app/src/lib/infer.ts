// Works out how a keyboard is wired from what the firmware's probe saw while
// the user pressed each key once.

import { LIMITS, type Contact, type DiodeDirection, type MatrixConfig, type Pin } from './protocol'

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
  /**
   * Positions the keyboard must not report: where a key without a diode in
   * a matrix read in both directions is read a second time.
   */
  doubles: number[]
  /** Things the user should know, in plain words. */
  notes: string[]
}

export type InferResult = { ok: true; wiring: Wiring } | { ok: false; problem: string }

type LinkKey = Extract<KeySignature, { kind: 'link' }>

/** One group of switches and, for each of its keys in press order, the position within the group. */
interface Part {
  config: MatrixConfig
  /** Index into the pressed keys, and where that key sits in this group. */
  keys: { key: number; local: number }[]
  size: number
  /** Positions within the group that read a key a second time. */
  doubles: number[]
  notes: string[]
}

/** The most switch positions one board can have (the firmware's limit). */
const MAX_POSITIONS = 128

/**
 * A board may mix styles: matrices, keys that each connect a pin of their
 * own to ground, and keys that connect one to supply voltage. Each is a
 * group, and the groups take their positions one after the other, the
 * matrices first. Keys that join two pins and share none with the rest (the
 * push switch of a knob on two pins of its own, say) are a matrix of their
 * own, as long as the keyboard has room for another group (`groups` in all).
 */
export function inferWiring(keys: KeySignature[], groups: number = LIMITS.moreWiring + 1): InferResult {
  if (keys.length === 0) return { ok: false, problem: 'No keys were pressed yet.' }
  const indexed = keys.map((key, index) => ({ key, index }))
  const parts: Part[] = []

  const direct = (['ground', 'supply'] as const).map((kind) => indexed.filter(({ key }) => key.kind === kind)).filter((group) => group.length)
  const links = indexed.filter(({ key }) => key.kind === 'link') as { key: LinkKey; index: number }[]
  const islands = islandsOf(links)
  // More islands than groups to read them in: the smallest share a matrix,
  // which then has crossings without a key.
  const room = Math.max(1, groups - direct.length)
  const shared = islands.length > room
  const matrices = shared ? [...islands.slice(0, room - 1), islands.slice(room - 1).flat()] : islands
  for (const [n, group] of matrices.entries()) {
    const matrix = inferMatrix(group.map(({ key }) => key))
    if ('problem' in matrix) return { ok: false, problem: matrix.problem }
    if (shared && n === matrices.length - 1) matrix.notes.push('Its keys are on pins apart from each other, so some of its crossings have no key.')
    parts.push({ ...matrix, keys: group.map(({ index }, i) => ({ key: index, local: matrix.positions[i] })) })
  }
  for (const group of direct) {
    const activeHigh = group[0].key.kind === 'supply'
    parts.push({
      config: { Direct: { pins: group.map(({ key }) => (key as { pin: Pin }).pin), active_high: activeHigh } },
      keys: group.map(({ index }, local) => ({ key: index, local })),
      size: group.length,
      doubles: [],
      notes: [
        activeHigh
          ? `${count(group.length)} its own pin to supply voltage.`
          : `${count(group.length)} its own pin to ground. Simple and reliable; it uses one pin per key.`,
      ],
    })
  }

  const positions: number[] = Array(keys.length).fill(0)
  const doubles: number[] = []
  let base = 0
  for (const part of parts) {
    for (const { key, local } of part.keys) positions[key] = base + local
    doubles.push(...part.doubles.map((local) => base + local))
    base += part.size
  }
  if (base > MAX_POSITIONS) return { ok: false, problem: `This wiring has room for ${base} keys, but a board can have ${MAX_POSITIONS}.` }
  if (doubles.length > LIMITS.doubles) {
    return {
      ok: false,
      problem: `${doubles.length} keys without a diode sit in a matrix whose diodes point both ways; a board can have ${LIMITS.doubles}. Check the diodes of the other keys there.`,
    }
  }
  const notes = parts.flatMap((part) => part.notes)
  if (parts.length > 1) notes.push('The keyboard mixes these wiring styles; each group is read in its own way.')
  return { ok: true, wiring: { matrix: parts[0].config, more: parts.slice(1).map((part) => part.config), positions, doubles, notes } }
}

const count = (keys: number) => (keys === 1 ? 'One key connects' : `${keys} keys each connect`)

/**
 * The keys in groups that share no pin with each other, each in press
 * order; the group with the most keys first.
 */
function islandsOf<T extends { key: LinkKey }>(keys: T[]): T[][] {
  const island = new Map<Pin, number>()
  const members = new Map<number, T[]>()
  keys.forEach((entry, n) => {
    const joined = [...new Set([island.get(entry.key.driven), island.get(entry.key.sensed)])].filter((id): id is number => id !== undefined)
    const mine = [...joined.flatMap((id) => members.get(id)!), entry]
    joined.forEach((id) => members.delete(id))
    members.set(n, mine)
    for (const { key } of mine) [key.driven, key.sensed].forEach((pin) => island.set(pin, n))
  })
  // Joining two islands puts the later one's keys behind the earlier one's.
  const order = new Map(keys.map((entry, n) => [entry, n]))
  return [...members.values()]
    .map((group) => group.sort((a, b) => order.get(a)! - order.get(b)!))
    .sort((a, b) => b.length - a.length || order.get(a[0])! - order.get(b[0])!)
}

function inferMatrix(
  keys: LinkKey[],
): { config: MatrixConfig; positions: number[]; size: number; doubles: number[]; notes: string[] } | { problem: string } {
  const withDiodes = keys.filter((key) => key.diode)
  const without = keys.length - withDiodes.length
  // Split the pins into two groups so that every key joins one pin of each:
  // give neighbouring pins opposite sides until every key is placed or two
  // pins clash. A part of the matrix not yet joined to the rest starts with
  // the pin that was pulled low on side 0, which is where every such pin
  // ends up when all diodes point the same way. It starts from a key with a
  // diode where there is one: only such a key says which pin that is.
  const side = new Map<Pin, 0 | 1>()
  const pending = new Set([...withDiodes, ...keys.filter((key) => !key.diode)])
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
  const backwards = (key: LinkKey) => key.diode && side.get(key.driven) === 1
  const duplex = keys.some(backwards)
  // The firmware drives rows for Col2Row and columns for Row2Col.
  const diodes: DiodeDirection = duplex ? 'Both' : drivenAreRows ? 'Col2Row' : 'Row2Col'
  const crossings = rows.length * cols.length
  const doubles: number[] = []
  const positions = keys.map((key) => {
    const pinOnSide = (which: 0 | 1) => (side.get(key.driven) === which ? key.driven : key.sensed)
    const row = rows.indexOf(pinOnSide(drivenAreRows ? 0 : 1))
    const col = cols.indexOf(pinOnSide(drivenAreRows ? 1 : 0))
    const crossing = row * cols.length + col
    // Read in both directions, a key without a diode shows at both positions
    // of its crossing. It is one key: the first position is its own.
    if (duplex && !key.diode) doubles.push(crossings + crossing)
    // In a duplex matrix the keys found by pulling a row low come first.
    const second = duplex && key.diode && !rows.includes(key.driven)
    return (second ? crossings : 0) + crossing
  })
  if (new Set([...positions, ...doubles]).size !== positions.length + doubles.length) {
    return { problem: 'Two keys join the same two pins the same way round. Check for keys wired in parallel.' }
  }
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`
  const size = crossings * (duplex ? 2 : 1)
  if (keys.length === 1 && crossings === 1) {
    const notes = [`One key joins two pins of its own${keys[0].diode ? ', through a diode' : ', without a diode'}.`]
    return { config: { Matrix: { rows, cols, diodes } }, positions, size, doubles, notes }
  }
  const notes = [`A matrix of ${plural(rows.length, 'row')} and ${plural(cols.length, 'column')}.`]
  if (duplex) {
    const few = keys.filter((key) => key.diode && rows.includes(key.driven) !== drivenAreRows).length
    notes.push(
      `The diodes point both ways (${few} of ${keys.length} keys the other way round), so the matrix is read in both directions. ` +
        'That is right for a duplex matrix with two keys per crossing; if yours is not one, those diodes are soldered backwards. ' +
        'Holding four or more keys at once can then make another appear pressed.',
    )
    if (without) {
      notes.push(
        `${without === 1 ? 'One key there has' : `${without} keys there have`} no diode and would show as both keys of a crossing; ` +
          `the keyboard reads ${without === 1 ? 'it' : 'each'} as one key.`,
      )
    }
  } else if (!without) notes.push('Every key has a diode, so any combination of keys can be held at once.')
  else if (!withDiodes.length) notes.push('The keys have no diodes. Holding three keys that form a corner can make a fourth appear pressed.')
  else {
    notes.push(
      `${without === 1 ? 'One key has' : `${without} keys have`} no diode, the others do. ` +
        `Holding ${without === 1 ? 'it' : 'one of them'} together with two keys that form a corner with it can make a fourth appear pressed.`,
    )
  }
  return { config: { Matrix: { rows, cols, diodes } }, positions, size, doubles, notes }
}
