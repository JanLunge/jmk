// One keyboard on the protocol level: requests with replies, and events.

import type {
  Action,
  Combo,
  Contact,
  DeviceEvent,
  DeviceMessage,
  HardwareConfig,
  HoldTapProfile,
  Info,
  KeyGeometry,
  MacroStep,
  Meta,
  Pin,
  Request,
  Response,
  SplitStatus,
} from './protocol'
import { describeError, LIMITS } from './protocol'
import type { Transport } from './transport'
import { bridge } from './wasm'

const REPLY_TIMEOUT_MS = 3000
let nextChannel = 1

export class KeyboardError extends Error {}

export class KeyboardClient {
  onKey: (pos: number, pressed: boolean) => void = () => {}
  onProbe: (contacts: Contact[]) => void = () => {}
  private readonly channel = nextChannel++
  private queue: Promise<unknown> = Promise.resolve()
  private waiting: ((response: Response) => void) | undefined

  constructor(readonly transport: Transport) {
    transport.onReport = (report) => this.receive(report)
  }

  private receive(report: number[]) {
    let decoded: { message: DeviceMessage } | null
    try {
      decoded = bridge({ op: 'decode', channel: this.channel, report })
    } catch {
      return // a damaged message; the request times out and can be retried
    }
    if (!decoded) return
    const message = decoded.message
    if ('Response' in message) {
      this.waiting?.(message.Response)
      this.waiting = undefined
    } else {
      this.dispatch(message.Event)
    }
  }

  private dispatch(event: DeviceEvent) {
    if ('Key' in event) this.onKey(event.Key.pos, event.Key.pressed)
    else this.onProbe(event.Probe)
  }

  /** Send a request and wait for its reply. Requests run one after another. */
  request(request: Request): Promise<Response> {
    const run = async () => {
      const { reports } = bridge<{ reports: number[][] }>({ op: 'encode', request })
      const reply = new Promise<Response>((resolve, reject) => {
        const timer = setTimeout(() => {
          this.waiting = undefined
          reject(new KeyboardError('The keyboard did not answer.'))
        }, REPLY_TIMEOUT_MS)
        this.waiting = (response) => {
          clearTimeout(timer)
          resolve(response)
        }
      })
      for (const report of reports) await this.transport.send(report)
      return reply
    }
    const result = this.queue.then(run, run)
    this.queue = result.catch(() => {})
    return result
  }

  private async expect<K extends string>(request: Request, key: K): Promise<any> {
    const response = await this.request(request)
    if (typeof response === 'object' && key in response) return (response as any)[key]
    throw this.failure(response)
  }

  private async ok(request: Request): Promise<void> {
    const response = await this.request(request)
    if (response !== 'Ok') throw this.failure(response)
  }

  private failure(response: Response): KeyboardError {
    if (typeof response === 'object' && 'Error' in response) return new KeyboardError(describeError(response.Error))
    return new KeyboardError('The keyboard sent an unexpected reply.')
  }

  info = (): Promise<Info> => this.expect('GetInfo', 'Info')
  hardware = (): Promise<HardwareConfig> => this.expect('GetHardware', 'Hardware')
  setHardware = (hardware: HardwareConfig) => this.ok({ SetHardware: hardware })
  layerCount = (): Promise<number> => this.expect('GetLayerCount', 'LayerCount')
  setLayerCount = (count: number) => this.ok({ SetLayerCount: count })
  profiles = (): Promise<HoldTapProfile[]> => this.expect('GetHoldTapProfiles', 'HoldTapProfiles')
  setProfile = (index: number, profile: HoldTapProfile) => this.ok({ SetHoldTapProfile: { index, profile } })
  combos = (): Promise<Combo[]> => this.expect('GetCombos', 'Combos')
  setCombos = (combos: Combo[]) => this.ok({ SetCombos: combos })
  meta = (): Promise<Meta> => this.expect('GetMeta', 'Meta')
  setMeta = (meta: Meta) => this.ok({ SetMeta: meta })
  splitStatus = (): Promise<SplitStatus> => this.expect('GetSplitStatus', 'SplitStatus')
  save = () => this.ok('Save')
  factoryReset = () => this.ok('FactoryReset')
  watchKeys = (on: boolean) => this.ok({ WatchKeys: on })
  startProbe = (pins: Pin[]) => this.ok({ StartProbe: pins })
  stopProbe = () => this.ok('StopProbe')
  reboot = (bootloader: boolean) => this.ok({ Reboot: { bootloader } })

  async actions(layer: number, count: number): Promise<Action[]> {
    const actions: Action[] = []
    while (actions.length < count) {
      const chunk: Action[] = await this.expect({ GetActions: { layer, pos: actions.length } }, 'Actions')
      if (chunk.length === 0) break
      actions.push(...chunk)
    }
    return actions.slice(0, count)
  }

  async setActions(layer: number, pos: number, actions: Action[]): Promise<void> {
    for (let i = 0; i < actions.length; i += LIMITS.actionsPerChunk) {
      await this.ok({ SetActions: { layer, pos: pos + i, actions: actions.slice(i, i + LIMITS.actionsPerChunk) } })
    }
  }

  async layout(): Promise<KeyGeometry[]> {
    const keys: KeyGeometry[] = []
    for (;;) {
      const chunk: { total: number; keys: KeyGeometry[] } = await this.expect(
        { GetLayout: { start: keys.length } },
        'Layout',
      )
      keys.push(...chunk.keys)
      if (keys.length >= chunk.total || chunk.keys.length === 0) return keys
    }
  }

  async setLayout(keys: KeyGeometry[]): Promise<void> {
    // The first chunk also truncates, so an empty layout needs one request too.
    for (let i = 0; i === 0 || i < keys.length; i += LIMITS.layoutKeysPerChunk) {
      await this.ok({ SetLayout: { start: i, keys: keys.slice(i, i + LIMITS.layoutKeysPerChunk) } })
    }
  }

  async macro(index: number): Promise<MacroStep[]> {
    const steps: MacroStep[] = []
    for (;;) {
      const chunk: { total: number; steps: MacroStep[] } = await this.expect(
        { GetMacro: { index, start: steps.length } },
        'Macro',
      )
      steps.push(...chunk.steps)
      if (steps.length >= chunk.total || chunk.steps.length === 0) return steps
    }
  }

  async setMacro(index: number, steps: MacroStep[]): Promise<void> {
    for (let i = 0; i === 0 || i < steps.length; i += LIMITS.macroStepsPerChunk) {
      await this.ok({ SetMacro: { index, start: i, steps: steps.slice(i, i + LIMITS.macroStepsPerChunk) } })
    }
  }

  async close(): Promise<void> {
    bridge({ op: 'close', channel: this.channel })
    await this.transport.close()
  }
}
