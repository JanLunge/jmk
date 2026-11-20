// Getting JMK firmware onto boards: new ones waiting in update mode, and
// keyboards that already run an older JMK.

import { computed, ref } from 'vue'
import { installVirtual, virtualDrives, type Bench } from './app'
import { desktop, type BootDrive } from './desktop'
import { registryMcu } from './registry'
import type { Session } from './session'
import { VirtualTransport } from './transport'

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

/**
 * Replace the firmware of a keyboard that runs JMK already. Its saved
 * settings stay. It drops off and comes back by itself.
 */
export async function updateFirmware(session: Session): Promise<void> {
  if (session.transport instanceof VirtualTransport) throw new Error('A virtual keyboard always runs the current firmware.')
  if (!desktop) throw new Error('Only the desktop app can update firmware by itself.')
  const before = new Set((await desktop.drives()).map((drive) => drive.path))
  session.restarting = true
  await session.client.reboot(true)
  try {
    // Its drive is the one that was not there a moment ago.
    let fresh: string | undefined
    for (let tries = 0; tries < 40 && !fresh; tries++) {
      await new Promise((resolve) => setTimeout(resolve, 400))
      fresh = (await desktop.drives()).find((drive) => !before.has(drive.path))?.path
    }
    await desktop.flash(registryMcu(session.info.mcu), fresh)
  } catch (error) {
    throw plain(error)
  }
}
