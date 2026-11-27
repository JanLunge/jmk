// Turns the stream of probe results into a list of distinct keys while the
// user presses them one after another.

import { contactKey, sameKey, signatureOf, type KeySignature } from './infer'
import type { Contact, Pin } from './protocol'

/** Contacts present this long after probing starts are not keys. */
const SETTLE_MS = 500
/**
 * A key without a diode conducts both ways, but the keyboard may notice one
 * way a moment before the other. The second way showing up this soon, while
 * the first is still there, is the same key and not the next one.
 */
const SAME_PRESS_MS = 150

export class KeyCollector {
  keys: KeySignature[] = []
  /** Connections that were there before any key was pressed. */
  stuck: Contact[] = []
  /** Pins with a connection right now. */
  active = new Set<Pin>()
  /** Index of the key pressed most recently, new or already known. */
  last: number | undefined
  /** Set when the last press could not be read as one key. */
  confused = false
  /** How many presses were read as one key so far. */
  presses = 0
  private previous = new Set<string>()
  /** When the last new key was found. */
  private foundAt = -Infinity
  private readonly startedAt: number

  constructor(private readonly now: () => number = () => performance.now()) {
    this.startedAt = now()
  }

  update(contacts: Contact[]) {
    if (this.now() - this.startedAt < SETTLE_MS) {
      this.stuck = contacts
      return
    }
    const ignored = new Set(this.stuck.map(contactKey))
    const current = contacts.filter((contact) => !ignored.has(contactKey(contact)))
    const fresh = current.filter((contact) => !this.previous.has(contactKey(contact)))
    const held = this.previous
    this.previous = new Set(current.map(contactKey))
    this.active = new Set(current.flatMap(pinsOf))
    if (!fresh.length) return

    if (fresh.length === 1 && 'Link' in fresh[0] && this.last !== undefined && this.now() - this.foundAt < SAME_PRESS_MS) {
      const { driven, sensed } = fresh[0].Link
      const key = this.keys[this.last]
      const reverse: Contact = { Link: { driven: sensed, sensed: driven } }
      const newest = key.kind === 'link' && key.diode && key.driven === sensed && key.sensed === driven
      // Not when this way round is a key found before: then they are the two keys of a duplex crossing.
      const apart = this.keys.some((other) => other.kind === 'link' && other.diode && other.driven === driven && other.sensed === sensed)
      if (newest && !apart && held.has(contactKey(reverse))) {
        // The other way round of the key just found: it has no diode.
        this.keys[this.last] = signatureOf([fresh[0], reverse])!
        this.confused = false
        return
      }
    }
    const signature = signatureOf(fresh)
    this.confused = !signature
    if (!signature) return
    this.presses++
    // A key known to have no diode is the same key when seen one way only.
    const plain = (key: KeySignature) =>
      key.kind === 'link' && !key.diode && signature.kind === 'link' && key.driven === signature.sensed && key.sensed === signature.driven
    const known = this.keys.findIndex((key) => sameKey(key, signature) || plain(key))
    if (known >= 0) {
      this.last = known
    } else {
      this.keys.push(signature)
      this.last = this.keys.length - 1
      this.foundAt = this.now()
    }
  }

  remove(index: number) {
    this.keys.splice(index, 1)
    this.last = undefined
  }
}

export function pinsOf(contact: Contact): Pin[] {
  if ('Ground' in contact) return [contact.Ground]
  if ('Supply' in contact) return [contact.Supply]
  return [contact.Link.driven, contact.Link.sensed]
}

/**
 * Finds the two signal pins of a rotary knob: while it is turned, both
 * touch ground again and again.
 */
export class KnobFinder {
  private readonly hits = new Map<Pin, number>()
  private previous = new Set<Pin>()

  constructor(private readonly exclude: Set<Pin>) {}

  /** Returns the two pins once both were seen often enough. */
  update(contacts: Contact[]): [Pin, Pin] | undefined {
    const grounded = new Set(contacts.flatMap((c) => ('Ground' in c && !this.exclude.has(c.Ground) ? [c.Ground] : [])))
    for (const pin of grounded) if (!this.previous.has(pin)) this.hits.set(pin, (this.hits.get(pin) ?? 0) + 1)
    this.previous = grounded
    const busy = [...this.hits].filter(([, count]) => count >= 2).map(([pin]) => pin)
    return busy.length >= 2 ? [busy[0], busy[1]] : undefined
  }
}
