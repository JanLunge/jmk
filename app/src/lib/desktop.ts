// What the desktop (Electron) shell adds to the page; see electron/preload.cjs.

/** A board in update mode: it shows up as a drive that takes a firmware file. */
export interface BootDrive {
  path: string
  /** What the board calls itself, e.g. "Raspberry Pi RP2". */
  model: string
  /** The chip, where the drive's description tells: 'rp2040', 'nrf52840' or ''. */
  mcu: string
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
}

export const desktop: Desktop | undefined = typeof window === 'undefined' ? undefined : (window as unknown as { jmkDesktop?: Desktop }).jmkDesktop
