// One keyboard on the protocol level: requests with replies, and events.

import type {
  Action,
  Combo,
  ComboOptions,
  Contact,
  DeviceEvent,
  DeviceMessage,
  HardwareConfig,
  HoldTapProfile,
  Info,
  KeyGeometry,
  Indicators,
  LightingConfig,
  MacroStep,
  MatrixConfig,
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
/** Protocol version from which a board can mix wiring styles and scan a duplex matrix. */
export const MORE_WIRING_PROTOCOL = 9
/** Protocol version from which a combo can be a sequence, be limited to layers and have more than four keys. */
export const COMBO_OPTIONS_PROTOCOL = 11
/** From which a board has eight groups of switches, not four, and reads a key without a diode in a duplex matrix as one key. */
export const MANY_GROUPS_PROTOCOL = 11
/** Groups of switches a keyboard of that protocol version can have besides its first. */
export const moreWiringLimit = (protocol: number) => (protocol < MORE_WIRING_PROTOCOL ? 0 : protocol < MANY_GROUPS_PROTOCOL ? 3 : LIMITS.moreWiring)
let nextChannel = 1

export class KeyboardError extends Error {}

export class KeyboardClient {
  onKey: (pos: number, pressed: boolean) => void = () => {}
  onProbe: (contacts: Contact[]) => void = () => {}
  onResolved: (pos: number, layer: number) => void = () => {}
  onLayers: (layers: number) => void = () => {}
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
    else if ('Probe' in event) this.onProbe(event.Probe)
    else if ('Resolved' in event) this.onResolved(event.Resolved.pos, event.Resolved.layer)
    else if ('Layers' in event) this.onLayers(event.Layers)
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

  /**
   * The other half of a split. Both halves hold the same keymap, so every
   * change to it is sent to both; `onMirrored` tells when that happened.
   */
  mirror?: KeyboardClient
  onMirrored: () => void = () => {}

  /** Send a change to the keymap, to the other half as well if there is one. */
  private async shared(request: Request): Promise<void> {
    await this.ok(request)
    const other = this.mirror
    if (!other) return
    try {
      await other.ok(request)
      this.onMirrored()
    } catch {
      // Unplugged, or busy restarting: the halves are brought level again
      // when both are saved.
    }
  }

  private async ok(request: Request): Promise<void> {
    const response = await this.request(request)
    if (response !== 'Ok') throw this.failure(response)
  }

  private failure(response: Response): KeyboardError {
    if (typeof response === 'object' && 'Error' in response) return new KeyboardError(describeError(response.Error))
    return new KeyboardError('The keyboard sent an unexpected reply.')
  }

  /** What the keyboard's firmware understands; known once `info` was asked. */
  private protocol = 0

  async info(): Promise<Info> {
    const info: Info = await this.expect('GetInfo', 'Info')
    this.protocol = info.protocol
    return info
  }

  /** The wiring, with the groups of switches beyond the first where the firmware knows of them. */
  async hardware(): Promise<HardwareConfig> {
    const hardware: HardwareConfig = await this.expect('GetHardware', 'Hardware')
    if (this.protocol < MORE_WIRING_PROTOCOL) return hardware
    const more: MatrixConfig[] = await this.expect('GetMoreWiring', 'MoreWiring')
    if (more.length) hardware.more = more
    if (this.protocol < MANY_GROUPS_PROTOCOL) return hardware
    const doubles: number[] = await this.expect('GetDoubles', 'Doubles')
    if (doubles.length) hardware.doubles = doubles
    return hardware
  }

  async setHardware(hardware: HardwareConfig): Promise<void> {
    const { more = [], doubles = [], ...first } = hardware
    const matrix = first.matrix
    const duplex = [matrix, ...more].some((part) => part !== 'None' && 'Matrix' in part && part.Matrix.diodes === 'Both')
    const tooOld = new KeyboardError("This keyboard's firmware is too old for this wiring. Update the firmware first.")
    if (this.protocol < MORE_WIRING_PROTOCOL) {
      if (more.length || duplex) throw tooOld
      return this.ok({ SetHardware: first })
    }
    if (more.length > moreWiringLimit(this.protocol) || (doubles.length && this.protocol < MANY_GROUPS_PROTOCOL)) throw tooOld
    // The keyboard checks each part against what it holds, so the old extra
    // groups go first: their pins may be wanted elsewhere now.
    const before: MatrixConfig[] = await this.expect('GetMoreWiring', 'MoreWiring')
    const doublesBefore: number[] = this.protocol < MANY_GROUPS_PROTOCOL ? [] : await this.expect('GetDoubles', 'Doubles')
    if (before.length) await this.ok({ SetMoreWiring: [] })
    try {
      await this.ok({ SetHardware: first })
      if (more.length) await this.ok({ SetMoreWiring: more })
      // Last: the keyboard forgets them whenever the wiring is set.
      if (doubles.length) await this.ok({ SetDoubles: doubles })
    } catch (error) {
      if (before.length) await this.ok({ SetMoreWiring: before }).catch(() => {})
      if (doublesBefore.length) await this.ok({ SetDoubles: doublesBefore }).catch(() => {})
      throw error
    }
  }
  layerCount = (): Promise<number> => this.expect('GetLayerCount', 'LayerCount')
  setLayerCount = (count: number) => this.shared({ SetLayerCount: count })
  profiles = (): Promise<HoldTapProfile[]> => this.expect('GetHoldTapProfiles', 'HoldTapProfiles')
  setProfile = (index: number, profile: HoldTapProfile) => this.shared({ SetHoldTapProfile: { index, profile } })
  async combos(): Promise<Combo[]> {
    const combos: Combo[] = await this.expect('GetCombos', 'Combos')
    if (this.protocol < COMBO_OPTIONS_PROTOCOL) return combos
    const options: ComboOptions[] = await this.expect('GetComboOptions', 'ComboOptions')
    return combos.map((combo, index) => ({
      ...combo,
      ...(options[index]?.layers ? { layers: options[index].layers } : {}),
      ...(options[index]?.sequence ? { sequence: true } : {}),
    }))
  }

  async setCombos(combos: Combo[]): Promise<void> {
    const plain = ({ keys, action, timeout_ms }: Combo) => ({ keys, action, timeout_ms })
    if (this.protocol < COMBO_OPTIONS_PROTOCOL) {
      // Older firmware would run a sequence as a chord and a combo for one
      // layer on all of them, and refuses long ones: leave those out.
      const known = combos.filter((combo) => !combo.sequence && !combo.layers && combo.keys.length <= LIMITS.comboKeysBefore)
      return this.shared({ SetCombos: known.map(plain) })
    }
    await this.shared({ SetCombos: combos.map(plain) })
    await this.shared({ SetComboOptions: combos.map((combo) => ({ layers: combo.layers ?? 0, sequence: combo.sequence ?? false })) })
  }
  meta = (): Promise<Meta> => this.expect('GetMeta', 'Meta')
  setMeta = (meta: Meta) => this.ok({ SetMeta: meta })
  splitStatus = (): Promise<SplitStatus> => this.expect('GetSplitStatus', 'SplitStatus')
  /** Settle a conflict between the halves: this half's keymap for both, or the other's. */
  resolveSplit = (keepThis: boolean) => this.ok({ ResolveSplit: { keep_this: keepThis } })
  splitAgreed = (digest: number, revision: number) => this.ok({ SplitAgreed: { digest, revision } })
  save = () => this.ok('Save')
  factoryReset = () => this.ok('FactoryReset')
  watchKeys = (on: boolean) => this.ok({ WatchKeys: on })
  startProbe = (pins: Pin[]) => this.ok({ StartProbe: pins })
  stopProbe = () => this.ok('StopProbe')
  holdLow = (pin: Pin | null) => this.ok({ HoldLow: pin })
  lighting = (): Promise<LightingConfig> => this.expect('GetLighting', 'Lighting')
  setLighting = (lighting: LightingConfig) => this.ok({ SetLighting: lighting })
  identifyLed = (led: number | null) => this.ok({ IdentifyLed: led })
  indicators = (): Promise<Indicators> => this.expect('GetIndicators', 'Indicators')
  setIndicators = (indicators: Indicators) => this.ok({ SetIndicators: indicators })
  layerColors = (): Promise<number[]> => this.expect('GetLayerColors', 'LayerColors')
  setLayerColors = (colors: number[]) => this.shared({ SetLayerColors: colors })
  macroName = (index: number): Promise<string> => this.expect({ GetMacroName: { index } }, 'MacroName')
  setMacroName = (index: number, name: string) => this.shared({ SetMacroName: { index, name } })
  letterRows = (): Promise<number[][]> => this.expect('GetLetterRows', 'LetterRows')
  setLetterRows = (rows: number[][]) => this.shared({ SetLetterRows: rows })
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
      await this.shared({ SetActions: { layer, pos: pos + i, actions: actions.slice(i, i + LIMITS.actionsPerChunk) } })
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
      await this.shared({ SetLayout: { start: i, keys: keys.slice(i, i + LIMITS.layoutKeysPerChunk) } })
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
      await this.shared({ SetMacro: { index, start: i, steps: steps.slice(i, i + LIMITS.macroStepsPerChunk) } })
    }
  }

  async close(): Promise<void> {
    bridge({ op: 'close', channel: this.channel })
    await this.transport.close()
  }
}
