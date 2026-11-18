// App-wide state: which keyboards are open and which one is in front.

import { computed, reactive } from 'vue'
import { DEMOS, type Demo } from './demos'
import { openSession, type Session } from './session'
import { chooseUsbDevice, knownUsbDevices, UsbTransport, VirtualTransport } from './transport'

interface Toast {
  id: number
  text: string
  kind: 'error' | 'ok'
}

export const app = reactive({
  sessions: [] as Session[],
  currentId: undefined as string | undefined,
  /** Sessions that belong together as halves of one demo split. */
  pairs: [] as [string, string][],
  toasts: [] as Toast[],
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

export async function openDemo(demo: Demo): Promise<void> {
  await attempt(async () => {
    const serial = ++demoCounter
    const transports = demo.halves.map(
      (half, index) => new VirtualTransport(demo.halves.length > 1 ? half.label : demo.name, half.wiring, serial * 2 + index),
    )
    if (transports.length === 2) {
      // Each half's sending pin is soldered to the other half's receiving pin.
      const [left, right] = demo.halves.map((half) => half.link)
      const wires: [number, number][] = left && right ? [[left.tx, right.rx], [left.rx, right.tx]] : []
      VirtualTransport.connect(transports[0], transports[1], wires)
    }
    const sessions: Session[] = []
    for (const [index, transport] of transports.entries()) sessions.push(await openSession(transport, demo.halves[index]))
    app.sessions.push(...sessions)
    if (sessions.length === 2) app.pairs.push([sessions[0].id, sessions[1].id])
    app.currentId = sessions[0].id
  })
}

export function demoById(id: string): Demo | undefined {
  return DEMOS.find((demo) => demo.id === id)
}

/** Record that two open keyboards are the halves of one split. */
export function pairUp(a: Session, b: Session): void {
  if (partnerOf(a) === b) return
  app.pairs = app.pairs.filter((pair) => !pair.includes(a.id) && !pair.includes(b.id))
  app.pairs.push([a.id, b.id])
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
export function addSession(session: Session): void {
  const index = app.sessions.findIndex((known) => sameBoard(known, session))
  if (index < 0) {
    app.sessions.push(session)
    app.currentId = session.id
    return
  }
  const old = app.sessions[index]
  // The same id keeps its place in the list, its split pairing and, if it
  // was in front, the view.
  session.id = old.id
  app.sessions.splice(index, 1, session)
  app.currentId ??= session.id
  old.client.close().catch(() => {})
}

async function openUsb(device: HIDDevice): Promise<void> {
  addSession(await openSession(await UsbTransport.open(device)))
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

export async function closeSession(session: Session): Promise<void> {
  const index = app.sessions.indexOf(session)
  if (index >= 0) app.sessions.splice(index, 1)
  app.pairs = app.pairs.filter((pair) => !pair.includes(session.id))
  if (app.currentId === session.id) app.currentId = app.sessions[0]?.id
  await session.client.close().catch(() => {})
}
