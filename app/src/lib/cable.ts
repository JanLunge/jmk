// Finds how the cable between two halves of a split keyboard is wired, with
// both halves plugged into the computer: one half holds its pins low one
// after the other while the other half watches which of its own pins goes
// low with it.

import type { Contact, Mcu, Pin } from './protocol'
import type { Session } from './session'

/** One data wire of the cable: a pin of the first half and the pin of the second half it reaches. */
export interface CableWire {
  a: Pin
  b: Pin
}

export interface LinkPins {
  tx: Pin
  rx: Pin
}

/** How long the watching half needs to notice a change: two passes over its pins and the way back. */
const NOTICE_MS = 90

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

/** Pins a wiring-detection result shows as tied to ground or supply. */
function fixed(contacts: Contact[]): Set<Pin> {
  return new Set(contacts.flatMap((c) => ('Ground' in c ? [c.Ground] : 'Supply' in c ? [c.Supply] : [])))
}

/**
 * The wires between `a` and `b`. `pinsA` and `pinsB` are the pins that may
 * be touched on each half. Neither half types while this runs (a few seconds).
 * `wait` is replaceable for tests that run on virtual time.
 */
export async function findWires(
  a: Session,
  b: Session,
  pinsA: Pin[],
  pinsB: Pin[],
  wait: (ms: number) => Promise<void> = sleep,
  progress: (done: number, total: number) => void = () => {},
): Promise<CableWire[]> {
  let seenA: Contact[] = []
  let seenB: Contact[] = []
  const handlers = [a.client.onProbe, b.client.onProbe]
  a.client.onProbe = (contacts) => (seenA = contacts)
  b.client.onProbe = (contacts) => (seenB = contacts)
  try {
    // Make the second half let go of its pins (a serial port it may have
    // on them would hold its sending pin high), then leave them floating.
    await b.client.startProbe(pinsB)
    await b.client.stopProbe()
    // Pins of the first half that are tied to supply or ground must not be
    // driven and cannot be cable wires.
    await a.client.startProbe(pinsA)
    await wait(2 * NOTICE_MS)
    await a.client.stopProbe()
    const skip = fixed(seenA)
    // The second half watches; what it sees grounded already is not the cable.
    seenB = []
    await b.client.startProbe(pinsB)
    await wait(2 * NOTICE_MS)
    const already = fixed(seenB)

    const wires: CableWire[] = []
    const sweep = pinsA.filter((pin) => !skip.has(pin))
    for (const [index, pin] of sweep.entries()) {
      progress(index, sweep.length)
      await a.client.holdLow(pin)
      await wait(NOTICE_MS)
      for (const reached of fixed(seenB)) {
        if (!already.has(reached)) wires.push({ a: pin, b: reached })
      }
    }
    return wires
  } finally {
    ;[a.client.onProbe, b.client.onProbe] = handlers
    await a.client.holdLow(null).catch(() => {})
    await b.client.stopProbe().catch(() => {})
  }
}

// The RP2040 can put a serial port only on certain pins; a pin pair must
// belong to the same port. Mirrors firmware/rp2040/src/link.rs.
const RP_PORTS: { tx: Pin[]; rx: Pin[] }[] = [
  { tx: [0, 12, 16, 28], rx: [1, 13, 17, 29] },
  { tx: [4, 8, 20, 24], rx: [5, 9, 21, 25] },
]

/** Whether this chip can send on `tx` and receive on `rx`. */
export function canLink(mcu: Mcu, tx: Pin, rx: Pin): boolean {
  if (tx === rx) return false
  if (mcu === 'Nrf52840') return true
  return RP_PORTS.some((port) => port.tx.includes(tx) && port.rx.includes(rx))
}

/** With `shared` both directions run over one wire: `tx` and `rx` are the same pin. */
export type LinkChoice = { ok: true; a: LinkPins; b: LinkPins; shared?: true } | { ok: false; problem: string }

/**
 * Pick which wire carries which direction. `name` turns a pin into the
 * label printed on the board, for the messages. `oneWire` says that both
 * halves can take turns on a single wire.
 */
export function chooseLink(
  wires: CableWire[],
  mcuA: Mcu,
  mcuB: Mcu,
  name: (half: 'a' | 'b', pin: Pin) => string = (_, pin) => `pin ${pin}`,
  oneWire = false,
): LinkChoice {
  const share = (wire: CableWire): LinkChoice => ({ ok: true, a: { tx: wire.a, rx: wire.a }, b: { tx: wire.b, rx: wire.b }, shared: true })
  if (wires.length === 0) {
    return { ok: false, problem: 'No wire was found between the two halves. Check that the cable is plugged in at both ends.' }
  }
  if (wires.length === 1) {
    const [wire] = wires
    if (oneWire) return share(wire)
    const found = `Only one data wire was found (${name('a', wire.a)} to ${name('b', wire.b)}).`
    return {
      ok: false,
      problem:
        mcuA === 'Rp2040' && mcuB === 'Rp2040'
          ? `${found} Newer firmware can use a single wire: update the firmware of both halves, then try again.`
          : `${found} On these chips JMK needs two: one for each direction.`,
    }
  }
  for (const out of wires) {
    for (const back of wires) {
      // `out` carries a to b, `back` carries b to a.
      if (out === back || out.a === back.a || out.b === back.b) continue
      if (canLink(mcuA, out.a, back.a) && canLink(mcuB, back.b, out.b)) {
        return { ok: true, a: { tx: out.a, rx: back.a }, b: { tx: back.b, rx: out.b } }
      }
    }
  }
  // No pair of them suits a serial port, but one wire alone can do both directions.
  if (oneWire) return share(wires[0])
  const list = wires.map((wire) => `${name('a', wire.a)} to ${name('b', wire.b)}`).join(', ')
  return {
    ok: false,
    problem:
      `The cable was found (${list}), but these chips cannot run a serial connection on those pins. ` +
      'An RP2040 sends on GP0, 12, 16 or 28 and receives on GP1, 13, 17 or 29, or sends on GP4, 8, 20 or 24 and ' +
      'receives on GP5, 9, 21 or 25. Move the two data wires to such a pair on each half.',
  }
}
