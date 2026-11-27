// App-wide state: which keyboards are open and which one is in front.

import { computed, markRaw, reactive, type Raw } from 'vue'
import { DEMOS, type Demo, type DemoHalf } from './demos'
import type { HostEvent, SplitStatus } from './protocol'
import { alignHalves, isConfigured, openSession, refresh, role, save, type Session } from './session'
import { PEER_PROTOCOL } from './client'
import { CableTransport, chooseUsbDevice, knownUsbDevices, UsbTransport, VirtualTransport } from './transport'

interface Toast {
  id: number
  text: string
  kind: 'error' | 'ok'
}

/** One board on the virtual workbench, with its USB cable to the computer. */
export interface BenchHalf {
  demo: DemoHalf
  transport: Raw<VirtualTransport>
  /** Its USB cable is in the computer. */
  usb: boolean
  /** JMK is on it. Without, it waits in update mode like a controller fresh from the shop. */
  firmware: boolean
  /** How far a firmware copy has got, 0 to 1, while one runs. */
  installing?: number
  /** The session it opened as, once it has. */
  sessionId?: string
}

/** A virtual keyboard as it lies on the desk: its boards and the cables that can be pulled. */
export interface Bench {
  id: string
  demo: Demo
  halves: BenchHalf[]
  /** The cable between the halves is plugged in. */
  cable: boolean
  /** What its computer received, newest last, and from which board. */
  typed: { half: number; event: HostEvent }[]
}

export const app = reactive({
  sessions: [] as Session[],
  benches: [] as Bench[],
  currentId: undefined as string | undefined,
  /** Sessions that belong together as halves of one demo split. */
  pairs: [] as [string, string][],
  toasts: [] as Toast[],
  /** Firmware is being put on boards: those that come back open quietly until all are done. */
  holding: false,
  busy: false,
})

export const current = computed(() => app.sessions.find((s) => s.id === app.currentId))

/** The other half of a split, if it is open too. */
export function partnerOf(session: Session): Session | undefined {
  const pair = app.pairs.find((p) => p.includes(session.id))
  const otherId = pair?.find((id) => id !== session.id)
  return app.sessions.find((s) => s.id === otherId)
}

let toastId = 0
export function toast(text: string, kind: Toast['kind'] = 'ok') {
  const id = ++toastId
  app.toasts.push({ id, text, kind })
  setTimeout(() => {
    const index = app.toasts.findIndex((t) => t.id === id)
    if (index >= 0) app.toasts.splice(index, 1)
  }, kind === 'error' ? 6000 : 2600)
}

/** Run something that talks to a keyboard and show any failure to the user. */
export async function attempt<T>(work: () => Promise<T>): Promise<T | undefined> {
  try {
    return await work()
  } catch (error) {
    toast(error instanceof Error ? error.message : String(error), 'error')
    return undefined
  }
}

let demoCounter = 0

/**
 * Put a virtual keyboard on the bench, the way a new build lies on the
 * desk: no firmware on its controller, no USB cable in yet, and for a split
 * only the cable between the halves. From there it goes through the same
 * setup as a real one.
 */
export async function openDemo(demo: Demo): Promise<void> {
  await attempt(async () => {
    const serial = ++demoCounter
    const split = demo.halves.length > 1
    const transports = demo.halves.map(
      (half, index) => new VirtualTransport(split ? half.label : demo.name, half.wiring, serial * 2 + index),
    )
    if (transports.length === 2) {
      // Each half's sending pin is soldered to the other half's receiving pin.
      const [left, right] = demo.halves.map((half) => half.link)
      // A cable with one data wire has the same pin for both directions.
      const wires: [number, number][] =
        left && right ? (left.tx === left.rx ? [[left.tx, right.rx]] : [[left.tx, right.rx], [left.rx, right.tx]]) : []
      VirtualTransport.connect(transports[0], transports[1], wires)
    }
    const bench: Bench = reactive({
      id: `bench${serial}`,
      demo,
      halves: demo.halves.map((half, index) => ({ demo: half, transport: markRaw(transports[index]), usb: false, firmware: false })),
      cable: true,
      typed: [],
    })
    // One build on the desk at a time: another that was never plugged in makes room.
    for (const waiting of app.benches.filter((each) => each.halves.every((half) => !half.sessionId))) {
      app.benches.splice(app.benches.indexOf(waiting), 1)
      for (const half of waiting.halves) await half.transport.close().catch(() => {})
    }
    app.benches.push(bench)
    power(bench)
    app.currentId = undefined
  })
}

export function benchOf(session: Session): Bench | undefined {
  return app.benches.find((bench) => bench.halves.some((half) => half.sessionId === session.id))
}

/** A board runs when it has firmware and gets power: from its own USB cable, or from the other half's through the cable between them. */
function power(bench: Bench): void {
  bench.halves.forEach((half, index) => {
    const other = bench.halves[1 - index]
    half.transport.setPowered(half.firmware && (half.usb || (bench.cable && !!other?.usb)))
  })
}

/** The computer sees a virtual board: open it like a keyboard that was just plugged in. */
async function attach(bench: Bench, half: BenchHalf): Promise<void> {
  half.transport = markRaw(half.transport.again())
  half.transport.setUsb(true)
  const session = await openSession(half.transport, half.demo)
  if (!half.usb) return
  const index = bench.halves.indexOf(half)
  const heard = half.transport.onHost
  half.transport.onHost = (event) => {
    heard(event)
    bench.typed.push({ half: index, event })
    if (bench.typed.length > 200) bench.typed.splice(0, bench.typed.length - 200)
  }
  addSession(session)
  half.sessionId = session.id
  const ids = bench.halves.map((each) => each.sessionId)
  if (ids.length === 2 && ids.every(Boolean) && !app.pairs.some((pair) => pair.includes(ids[0]!))) app.pairs.push(ids as [string, string])
  relink()
}

/** Plug a virtual board's USB cable in or pull it out. */
export async function plugUsb(bench: Bench, index: number, plugged: boolean): Promise<void> {
  const half = bench.halves[index]
  if (!half || half.usb === plugged) return
  half.usb = plugged
  if (!plugged) half.transport.unplug()
  power(bench)
  if (plugged && half.firmware) await attempt(() => attach(bench, half))
}

/** Plug the cable between two virtual halves in or pull it out. */
export function plugCable(bench: Bench, plugged: boolean): void {
  bench.cable = plugged
  bench.halves[0].transport.setCable(plugged)
  power(bench)
}

/** Virtual boards the computer sees in update mode, waiting for firmware. */
export const virtualDrives = computed(() =>
  app.benches.flatMap((bench) => bench.halves.map((half, index) => ({ bench, index, half })).filter(({ half }) => half.usb && !half.firmware)),
)

/** How long a virtual board takes to come back as a keyboard after its firmware went on, like a real one. */
export const RESTART_MS = 1800

/**
 * Copy the firmware onto a virtual board. As with a real one, this is done
 * when the copy is: the board then restarts and opens by itself a little later.
 */
export async function installVirtual(bench: Bench, index: number): Promise<void> {
  const half = bench.halves[index]
  if (!half || half.firmware || half.installing !== undefined) return
  half.installing = 0
  await new Promise<void>((done) => {
    const timer = setInterval(() => {
      half.installing = Math.min(1, (half.installing ?? 0) + 0.08)
      if (half.installing < 1) return
      clearInterval(timer)
      done()
    }, 60)
  })
  half.installing = undefined
  half.firmware = true
  setTimeout(() => {
    power(bench)
    if (half.usb) attempt(() => attach(bench, half))
  }, RESTART_MS)
}

export function demoById(id: string): Demo | undefined {
  return DEMOS.find((demo) => demo.id === id)
}

/** Record that two open keyboards are the halves of one split. */
export function pairUp(a: Session, b: Session): void {
  if (partnerOf(a) !== b) {
    app.pairs = app.pairs.filter((pair) => !pair.includes(a.id) && !pair.includes(b.id))
    app.pairs.push([a.id, b.id])
  }
  relink()
}

/** A half that is set up and can take changes to the keymap. */
const isReadyHalf = (session: Session) => !session.closed && !session.setup && isConfigured(session) && role(session) !== 'single'

/**
 * Both halves of a split hold the same keymap. While both are open, a
 * change made on one is sent to the other as well.
 */
export function relink(): void {
  for (const session of app.sessions) {
    const partner = partnerOf(session)
    const linked = partner && isReadyHalf(session) && isReadyHalf(partner)
    session.client.mirror = linked ? partner.client : undefined
    session.client.onMirrored = () => {
      if (!partner) return
      partner.stale = true
      partner.unsaved = true
    }
  }
}

const sameUid = (a: number[], b: number[]) => a.some((byte) => byte !== 0) && a.length === b.length && a.every((byte, i) => byte === b[i])

/** The sessions that make up one keyboard: the halves of a split together, the first half first. */
export function halvesOf(session: Session): Session[] {
  const partner = partnerOf(session)
  if (!partner) return [session]
  const pair = app.pairs.find((p) => p.includes(session.id))!
  return pair[0] === session.id ? [session, partner] : [partner, session]
}

/** Which half of its keyboard a board is, in the user's words. */
export const sideOf = (session: Session) => (role(session) === 'second' ? 'right' : 'left')

/** The two halves hear each other over their cable. */
export const cabled = (a: Session, b: Session) => a.split.connected && sameUid(a.split.peer_uid, b.info.uid)

// --- Keeping the halves' keymaps the same ------------------------------------
// The firmware does this over the cable between the halves (see sync.rs).
// Two halves that are both on USB but not joined cannot, so the app does it
// for them by the same rule.

type Verdict = 'same' | 'fetch' | 'offer' | 'conflict'

/** Whose keymap is the newer one, seen from `me`. The firmware's rule, word for word. */
export function decide(me: SplitStatus, peer: SplitStatus): Verdict {
  if (me.digest === peer.digest) return 'same'
  const iAmOld = peer.base === me.digest
  const peerIsOld = me.base === peer.digest
  if (iAmOld !== peerIsOld) return iAmOld ? 'fetch' : 'offer'
  const changed = (side: SplitStatus) => side.base !== null && side.base !== side.digest
  if (changed(me) !== changed(peer)) return changed(me) ? 'offer' : 'fetch'
  if (changed(me)) return 'conflict'
  if (me.revision === 0 && peer.revision > 0) return 'fetch'
  if (peer.revision === 0 && me.revision > 0) return 'offer'
  return 'conflict'
}

/** Tell both halves that they hold the same keymap, so the next change on one is known to be the newer. */
async function agree(a: Session, b: Session): Promise<void> {
  a.split = await a.client.splitStatus()
  b.split = await b.client.splitStatus()
  if (a.split.digest !== b.split.digest) return
  const revision = Math.max(a.split.revision, b.split.revision)
  for (const half of [a, b]) {
    if (half.split.base !== half.split.digest || half.split.revision !== revision) {
      await half.client.splitAgreed(half.split.digest, revision).catch(() => {})
    }
  }
}

/** Give `to` the keymap of `from` and save it there. */
async function copyKeymap(from: Session, to: Session): Promise<void> {
  // The cable may have changed either keymap since the app last read it.
  await refresh(from)
  to.stale = true
  await alignHalves(from, to)
  await save(to)
  await agree(from, to)
}

/**
 * Bring two halves that are both open level. Returns the half that was
 * brought up to date, 'conflict' if the user has to choose, else nothing.
 */
export async function reconcile(a: Session, b: Session): Promise<Session | 'conflict' | undefined> {
  if (!isReadyHalf(a) || !isReadyHalf(b) || a.unsaved || b.unsaved) return undefined
  a.split = await a.client.splitStatus()
  b.split = await b.client.splitStatus()
  if (a.split.sync === 'Conflict' || b.split.sync === 'Conflict') return 'conflict'
  // Joined by their cable they sort it out themselves.
  if (cabled(a, b)) return undefined
  const verdict = decide(a.split, b.split)
  if (verdict === 'conflict') return 'conflict'
  if (verdict === 'same') {
    await agree(a, b)
    return undefined
  }
  const [from, to] = verdict === 'offer' ? [a, b] : [b, a]
  await copyKeymap(from, to)
  return to
}

/** The user's answer to a conflict: `keep`'s keymap goes onto the other half. */
export async function resolveConflict(keep: Session): Promise<void> {
  const other = partnerOf(keep)
  if (other && !other.closed && !cabled(keep, other)) await copyKeymap(keep, other)
  else if (!keep.closed) await keep.client.resolveSplit(true)
  else if (other) await other.client.resolveSplit(false)
}

/** Pair a half with the open keyboard it hears over its cable, if that is not done yet. */
export function pairByCable(session: Session): void {
  if (partnerOf(session)) return
  const other = app.sessions.find((s) => s !== session && !s.closed && sameUid(session.split.peer_uid, s.info.uid))
  if (other && role(other) !== 'single') pairUp(session, other)
}

/**
 * Write a keyboard's settings to its flash. The other half of a split is
 * brought level first if it differs, and saved too.
 */
export async function saveAll(session: Session): Promise<void> {
  await save(session)
  const partner = partnerOf(session)
  if (!partner || !isReadyHalf(session) || !isReadyHalf(partner)) return
  await alignHalves(session, partner)
  if (partner.unsaved) await save(partner)
  // Joined by their cable the halves notice by themselves that they agree.
  session.split = await session.client.splitStatus()
  if (!cabled(session, partner)) await agree(session, partner)
}

/** Every controller reports its own serial number; all zeros means it could not be read. */
function sameBoard(a: Session, b: Session): boolean {
  const uid = a.info.uid
  return uid.some((byte) => byte !== 0) && uid.length === b.info.uid.length && uid.every((byte, i) => byte === b.info.uid[i])
}

/**
 * Show a freshly opened keyboard. One that was open before (unplugged and
 * back, or returned from a firmware update) takes its old place instead of
 * appearing a second time.
 */
export function addSession(session: Session, front = true): void {
  const index = app.sessions.findIndex((known) => sameBoard(known, session))
  if (index < 0) {
    app.sessions.push(session)
    // A keyboard in the middle of its setup stays in front: a second board
    // that appears joins that setup.
    if (front && !app.holding && (!current.value || current.value.closed || !current.value.setup)) app.currentId = session.id
    return
  }
  // Over USB a half answers faster than over the cable: a half open both
  // ways stays the USB one.
  if (session.transport.kind === 'cable') {
    session.client.close().catch(() => {})
    return
  }
  const old = app.sessions[index]
  // The same id keeps its place in the list, its split pairing and, if it
  // was in front, the view.
  session.id = old.id
  app.sessions.splice(index, 1, session)
  app.currentId ??= session.id
  old.client.close().catch(() => {})
  relink()
}

async function openUsb(device: HIDDevice): Promise<void> {
  const session = await openSession(await UsbTransport.open(device))
  addSession(session)
  openFarHalf(session)
}

// --- The other half, over the cable -------------------------------------------

/** When a half whose firmware did not answer over the cable may be tried again. */
const cableRetryAt = new Map<string, number>()
const CABLE_RETRY_MS = 30_000
const opening = new Set<string>()

/**
 * Open the other half of a split through this one and the cable between
 * them, if it is not open already: then it can be set up without a USB
 * cable of its own. Needs firmware on both halves that passes requests on.
 */
export async function openFarHalf(session: Session): Promise<void> {
  if (session.closed || session.transport.kind === 'cable' || session.info.protocol < PEER_PROTOCOL) return
  if (role(session) === 'single' || !session.split.connected) return
  const uid = session.split.peer_uid
  const key = uid.join(',')
  if (!uid.some((byte) => byte !== 0) || opening.has(key) || (cableRetryAt.get(key) ?? 0) > Date.now()) return
  if (app.sessions.some((each) => !each.closed && sameUid(each.info.uid, uid))) return
  opening.add(key)
  const transport = new CableTransport(session.client, `${session.transport.label} (over the cable)`)
  const closeBefore = session.transport.onClose
  session.transport.onClose = () => {
    closeBefore()
    transport.lost()
  }
  try {
    const far = await openSession(transport)
    if (session.closed || app.sessions.some((each) => !each.closed && sameBoard(each, far))) {
      await far.client.close().catch(() => {})
      return
    }
    addSession(far, false)
    pairUp(session, far)
  } catch {
    // Firmware on the other half from before it answered over the cable,
    // or the cable came out.
    transport.lost()
    cableRetryAt.set(key, Date.now() + CABLE_RETRY_MS)
  } finally {
    opening.delete(key)
  }
}

export async function connectUsb(): Promise<void> {
  await attempt(async () => {
    const device = await chooseUsbDevice()
    if (device) await openUsb(device)
  })
}

/** Open keyboards the user allowed before, without asking again. */
export async function reconnectUsb(): Promise<void> {
  for (const device of await knownUsbDevices()) {
    if (!device.opened) await attempt(() => openUsb(device))
  }
}

/** Close a keyboard: all of it, so both halves of a split and a virtual one's bench. */
export async function closeSession(session: Session): Promise<void> {
  const bench = benchOf(session)
  const closing = bench ? app.sessions.filter((each) => benchOf(each) === bench) : [session]
  for (const each of closing) {
    const index = app.sessions.indexOf(each)
    if (index >= 0) app.sessions.splice(index, 1)
    app.pairs = app.pairs.filter((pair) => !pair.includes(each.id))
  }
  if (bench) app.benches.splice(app.benches.indexOf(bench), 1)
  if (closing.some((each) => each.id === app.currentId)) app.currentId = app.sessions[0]?.id
  relink()
  for (const each of closing) await each.client.close().catch(() => {})
  if (bench) for (const half of bench.halves) await half.transport.close().catch(() => {})
}
