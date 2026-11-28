// Getting JMK firmware onto boards: new ones waiting in update mode, and
// keyboards that already run an older JMK.

import { computed, ref } from 'vue'
import { app, halvesOf, installVirtual, saveAll, sideOf, virtualDrives, type Bench } from './app'
import { desktop, type BootDrive } from './desktop'
import type { Mcu } from './protocol'
import type { Session } from './session'
import { bridge } from './wasm'

/** Protocol version from which the firmware can help find the cable between halves. */
export const CABLE_SEARCH_PROTOCOL = 2

export const canFindCable = (session: Session) => session.info.protocol >= CABLE_SEARCH_PROTOCOL

/** Boards attached in update mode, kept current while the desktop app runs. */
export const bootDrives = ref<BootDrive[]>([])

let watching = false
export function watchBootDrives(): void {
  if (!desktop || watching) return
  watching = true
  const look = async () => {
    try {
      bootDrives.value = await desktop!.drives()
    } catch {
      bootDrives.value = []
    }
  }
  look()
  setInterval(look, 1000)
}

/** A board the computer sees in update mode: a real one, or one on the virtual workbench. */
export interface Arrival {
  key: string
  model: string
  drive?: BootDrive
  virtual?: { bench: Bench; index: number }
  /** How far the firmware copy has got, 0 to 1, where that is known. */
  progress?: number
}

export const arrivals = computed<Arrival[]>(() => [
  ...bootDrives.value.map((drive) => ({ key: drive.path, model: drive.model, drive })),
  ...virtualDrives.value.map(({ bench, index, half }) => ({
    key: `${bench.id}:${index}`,
    model: 'Virtual controller',
    virtual: { bench, index },
    progress: half.installing,
  })),
])

/** Put JMK on a board that waits in update mode. */
export function installArrival(arrival: Arrival): Promise<void> {
  return arrival.virtual ? installVirtual(arrival.virtual.bench, arrival.virtual.index) : installOnBoard(arrival.drive)
}

const plain = (error: unknown) =>
  new Error((error instanceof Error ? error.message : String(error)).replace(/^Error invoking remote method '[^']+': Error: /, ''))

/** Put JMK on a board that is in update mode. It restarts as a keyboard and opens by itself. */
export async function installOnBoard(drive?: BootDrive): Promise<void> {
  if (!desktop) throw new Error('Only the desktop app can copy firmware onto a board.')
  try {
    await desktop.flash(drive?.mcu || 'rp2040', drive?.path)
  } catch (error) {
    throw plain(error)
  }
}

type Version = [number, number, number]

/** The firmware version this app carries: the one it was built together with. */
export const carriedVersion = (): Version => bridge<{ firmware: Version }>({ op: 'version' }).firmware

export const isOlder = (a: Version, b: Version) => a[0] !== b[0] ? a[0] < b[0] : a[1] !== b[1] ? a[1] < b[1] : a[2] < b[2]

/** The chips this app has a firmware file for. */
const carriedChips = ref<string[]>([])
desktop?.bundled().then((chips) => (carriedChips.value = chips), () => {})

const carried = (session: Session) => carriedChips.value.includes(chipName(session.info.mcu))
/** A half reached over the cable cannot go into update mode: that needs its own USB cable. */
const canUpdate = (session: Session) => session.transport.kind === 'usb' && carried(session)

/** The app carries newer firmware than this board runs, and can put it on by itself. */
export const updateDue = (session: Session) => canUpdate(session) && !session.closed && isOlder(session.info.firmware, carriedVersion())

/** A half reached over the cable runs older firmware; plugged in by USB it could be updated. */
export const updateNeedsUsb = (session: Session) =>
  session.transport.kind === 'cable' && carried(session) && !session.closed && isOlder(session.info.firmware, carriedVersion())

/** What an update in progress is doing, for the screen that shows while the keyboard is away. */
export const updating = ref<string>()

/**
 * Replace the firmware of one board that runs JMK already. Its saved
 * settings stay. It drops off and comes back by itself.
 */
async function updateBoard(session: Session): Promise<void> {
  if (!desktop) throw new Error('Only the desktop app can update firmware by itself.')
  const id = session.id
  const before = new Set((await desktop.drives()).map((drive) => drive.path))
  session.restarting = true
  try {
    await session.client.reboot(true)
    // Its drive is the one that was not there a moment ago.
    let fresh: string | undefined
    for (let tries = 0; tries < 40 && !fresh; tries++) {
      await new Promise((resolve) => setTimeout(resolve, 400))
      fresh = (await desktop.drives()).find((drive) => !before.has(drive.path))?.path
    }
    if (!fresh) throw new Error('The keyboard did not show up in update mode. Unplug it and plug it back in, then try again.')
    await desktop.flash(chipName(session.info.mcu), fresh)
    // It restarts with the new firmware and opens again under the same id.
    for (let tries = 0; ; tries++) {
      await new Promise((resolve) => setTimeout(resolve, 400))
      const back = app.sessions.find((each) => each.id === id && each !== session && !each.closed)
      if (back) return
      if (tries > 75) throw new Error('The firmware was copied, but the keyboard has not come back. Unplug it and plug it back in.')
    }
  } catch (error) {
    session.restarting = false
    throw plain(error)
  }
}

/**
 * Bring a keyboard up to the firmware this app carries, all of it: both
 * halves of a split, one after the other, as each one is away for a moment.
 * With `all` also boards that are current already, to put the firmware on anew.
 */
export async function updateFirmware(session: Session, all = false): Promise<void> {
  if (updating.value) return
  const ids = halvesOf(session)
    .filter((half) => !half.closed && (all ? canUpdate(half) : updateDue(half)))
    .map((half) => half.id)
  if (!ids.length) throw new Error('This app has no firmware for this keyboard.')
  try {
    updating.value = 'Saving your changes first…'
    if (halvesOf(session).some((half) => half.unsaved)) await saveAll(session)
    for (const [index, id] of ids.entries()) {
      // The first half came back as a new session; look each one up afresh.
      const half = app.sessions.find((each) => each.id === id && !each.closed)
      if (!half) throw new Error('A half was unplugged during the update.')
      updating.value = ids.length > 1 ? `Updating the ${sideOf(half)} half (${index + 1} of ${ids.length})…` : 'Updating the firmware…'
      await updateBoard(half)
    }
  } finally {
    updating.value = undefined
  }
}

/** The chip family as the firmware files are named; a virtual keyboard stands in for an RP2040. */
export function chipName(mcu: Mcu): string {
  return mcu === 'Simulated' ? 'rp2040' : mcu.toLowerCase()
}
