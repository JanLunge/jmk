// What the desktop (Electron) shell adds to the page; see electron/preload.cjs.

export interface Desktop {
  /** Copy the bundled firmware for `mcu` onto the bootloader drive. Resolves to the drive's path. */
  flash(mcu: string): Promise<string>
}

export const desktop: Desktop | undefined = (window as unknown as { jmkDesktop?: Desktop }).jmkDesktop
