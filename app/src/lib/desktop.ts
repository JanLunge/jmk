// What the desktop (Electron) shell adds to the page; see electron/preload.cjs.
import { ref } from 'vue'

/** A board in update mode: it shows up as a drive that takes a firmware file. */
export interface BootDrive {
  path: string
  /** What the board calls itself, e.g. "Raspberry Pi RP2". */
  model: string
  /** The chip, where the drive's description tells: 'rp2040', 'rp2350', 'nrf52840' or ''. */
  mcu: string
}

/** Where an update of the app itself stands; see electron/updater.cjs. */
export interface AppUpdate {
  /** 'off' means this build cannot update itself. */
  kind: 'off' | 'idle' | 'checking' | 'current' | 'downloading' | 'ready' | 'failed'
  /** The version that is running. */
  version: string
  /** The version being fetched, or waiting for a restart. */
  next?: string
  /** 0 to 1 while downloading. */
  progress?: number
  message?: string
}

export interface Desktop {
  /** Node's name for the operating system: 'darwin', 'win32', 'linux'. */
  platform: string
  /**
   * Copy the bundled firmware for `mcu` onto a bootloader drive, waiting a
   * while for one to appear. `onto` picks one drive. Resolves to its path.
   */
  flash(mcu: string, onto?: string): Promise<string>
  /** Boards attached in update mode right now. */
  drives(): Promise<BootDrive[]>
  /** The chips the app carries a firmware file for, e.g. ['rp2040']. */
  bundled(): Promise<string[]>
  update: {
    state(): Promise<AppUpdate>
    /** Look for a newer release now; a newer one is downloaded straight away. */
    check(): Promise<AppUpdate>
    /** Restart into the downloaded update. */
    install(): Promise<void>
    watch(listener: (state: AppUpdate) => void): void
  }
}

export const desktop: Desktop | undefined = typeof window === 'undefined' ? undefined : (window as unknown as { jmkDesktop?: Desktop }).jmkDesktop

/** The app's own update, kept current by the desktop shell. */
export const appUpdate = ref<AppUpdate>()
if (desktop?.update) {
  desktop.update.state().then((state) => (appUpdate.value = state))
  desktop.update.watch((state) => (appUpdate.value = state))
}
