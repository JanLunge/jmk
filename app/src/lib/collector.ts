// Turns the stream of probe results into a list of distinct keys while the
// user presses them one after another.

import { contactKey, sameKey, signatureOf, type KeySignature } from './infer'
import type { Contact, Pin } from './protocol'

/** Contacts present this long after probing starts are not keys. */
const SETTLE_MS = 500

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
  /** Contacts from `stuck` missing from the last probe pass. */
  private opened = new Set<string>()
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
    this.previous = new Set(current.map(contactKey))
    // A connection that was there at rest and lets go is a key that opens
    // when pressed: a light-beam sensor, like the centre key of a Svalboard
    // finger cluster.
    const present = new Set(contacts.map(contactKey))
    const opened = this.stuck.filter((contact) => !present.has(contactKey(contact)))
    const freshlyOpened = opened.filter((contact) => !this.opened.has(contactKey(contact)))
    this.opened = new Set(opened.map(contactKey))
    this.active = new Set([...current, ...opened].flatMap(pinsOf))
    if (!fresh.length && !freshlyOpened.length) return

    const closing = fresh.length ? signatureOf(fresh) : undefined
    const opening = freshlyOpened.length ? signatureOf(freshlyOpened) : undefined
    const signature = fresh.length && freshlyOpened.length ? undefined : (closing ?? (opening && { ...opening, inverted: true }))
    this.confused = !signature
    if (!signature) return
    this.presses++
    const known = this.keys.findIndex((key) => sameKey(key, signature))
    if (known >= 0) {
      this.last = known
    } else {
      this.keys.push(signature)
      this.last = this.keys.length - 1
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
