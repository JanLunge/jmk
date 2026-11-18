// How reports reach a keyboard: USB (WebHID) or a virtual keyboard running
// in the WebAssembly bridge. Both look the same to the rest of the app.

import type { HostEvent, Wiring } from './protocol'
import { bridge } from './wasm'

export interface Transport {
  readonly kind: 'usb' | 'virtual'
  readonly label: string
  send(report: number[]): Promise<void>
  /** Called for every 64-byte report from the keyboard. */
  onReport: (report: number[]) => void
  /** Called when the keyboard goes away. */
  onClose: () => void
  close(): Promise<void>
}

const VENDOR_USAGE_PAGE = 0xff4a
export const USB_FILTER = { usagePage: VENDOR_USAGE_PAGE }

export function usbSupported(): boolean {
  return 'hid' in navigator
}

export class UsbTransport implements Transport {
  readonly kind = 'usb'
  onReport: (report: number[]) => void = () => {}
  onClose: () => void = () => {}
  private readonly onDisconnect = (event: HIDConnectionEvent) => {
    if (event.device === this.device) this.onClose()
  }

  private constructor(private readonly device: HIDDevice) {
    device.addEventListener('inputreport', (event) => {
      this.onReport(Array.from(new Uint8Array(event.data.buffer, event.data.byteOffset, event.data.byteLength)))
    })
    navigator.hid.addEventListener('disconnect', this.onDisconnect)
  }

  get label(): string {
    return this.device.productName || 'JMK keyboard'
  }

  static async open(device: HIDDevice): Promise<UsbTransport> {
    if (!device.opened) await device.open()
    return new UsbTransport(device)
  }

  async send(report: number[]): Promise<void> {
    await this.device.sendReport(0, new Uint8Array(report))
  }

  async close(): Promise<void> {
    navigator.hid.removeEventListener('disconnect', this.onDisconnect)
    await this.device.close()
  }
}

/** Keyboards already allowed, plus whatever the user picks in the chooser. */
export async function knownUsbDevices(): Promise<HIDDevice[]> {
  if (!usbSupported()) return []
  const devices = await navigator.hid.getDevices()
  return devices.filter((d) => d.collections.some((c) => c.usagePage === VENDOR_USAGE_PAGE))
}

export async function chooseUsbDevice(): Promise<HIDDevice | undefined> {
  const [device] = await navigator.hid.requestDevice({ filters: [USB_FILTER] })
  return device
}

// --- Virtual keyboards -------------------------------------------------------

interface AdvanceResult {
  [id: string]: { reports: number[][]; host: HostEvent[]; bootloader: boolean; lights: number[][] | null }
}

const TICK_MS = 8

/** Owns the clock for all virtual keyboards so two halves stay in step. */
class VirtualWorld {
  private readonly keyboards = new Map<number, VirtualTransport>()
  private timer: ReturnType<typeof setInterval> | undefined

  add(keyboard: VirtualTransport) {
    this.keyboards.set(keyboard.id, keyboard)
    this.timer ??= setInterval(() => this.advance(TICK_MS), TICK_MS)
  }

  remove(id: number) {
    this.keyboards.delete(id)
    if (this.keyboards.size === 0) {
      clearInterval(this.timer)
      this.timer = undefined
    }
  }

  advance(ms: number) {
    if (this.keyboards.size === 0) return
    const result = bridge<AdvanceResult>({ op: 'advance', ids: [...this.keyboards.keys()], ms })
    for (const [id, keyboard] of this.keyboards) {
      const out = result[id]
      keyboard.bootloader = out.bootloader
      if (out.lights) keyboard.onLights(out.lights)
      out.host.forEach((event) => keyboard.onHost(event))
      out.reports.forEach((report) => keyboard.onReport(report))
    }
  }
}

const world = new VirtualWorld()

export class VirtualTransport implements Transport {
  readonly kind = 'virtual'
  readonly id: number
  bootloader = false
  onReport: (report: number[]) => void = () => {}
  onClose: () => void = () => {}
  /** What this keyboard "typed" into its computer. */
  onHost: (event: HostEvent) => void = () => {}
  /** The colours its light chain shows, as sent down the wire. */
  onLights: (colors: number[][]) => void = () => {}

  constructor(
    readonly label: string,
    readonly wiring: Wiring,
    uid: number,
  ) {
    this.id = bridge<{ id: number }>({ op: 'create', wiring, uid: [uid, 0x4a, 0x4d, 0x4b, 0, 0, 0, 1] }).id
    world.add(this)
  }

  /** Join two halves with a cable; `wires` lists which pin of `a` reaches which pin of `b`. */
  static connect(a: VirtualTransport, b: VirtualTransport, wires: [number, number][] = []) {
    bridge({ op: 'connect', a: a.id, b: b.id, wires })
  }

  async send(report: number[]): Promise<void> {
    bridge({ op: 'send', id: this.id, report })
  }

  setKey(key: number, pressed: boolean) {
    bridge({ op: 'key', id: this.id, key, pressed })
  }

  turnEncoder(encoder: number, clockwise: boolean) {
    bridge({ op: 'encoder', id: this.id, encoder, clockwise })
  }

  setCable(plugged: boolean) {
    bridge({ op: 'cable', id: this.id, plugged })
  }

  /** Complete a simulated firmware update. */
  finishFlash() {
    bridge({ op: 'flash', id: this.id })
  }

  async close(): Promise<void> {
    world.remove(this.id)
    bridge({ op: 'destroy', id: this.id })
  }
}

/** For tests: move virtual time forward without waiting. */
export function advanceVirtualTime(ms: number) {
  world.advance(ms)
}
