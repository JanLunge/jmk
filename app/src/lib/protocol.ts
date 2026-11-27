// The wire types of crates/jmk-proto as they appear in JSON. The Rust side
// (compiled to WebAssembly) does the binary encoding, so this file only has
// to match serde's JSON shape: unit variants are strings, other variants are
// objects with one key.

export type Pin = number

export type Basic =
  | 'None'
  | { Key: number }
  | { Mods: number }
  | { ModKey: { mods: number; key: number } }
  | { Consumer: number }
  | { MomentaryLayer: number }
  | { ToggleLayer: number }
  | { ToLayer: number }
  | { OneShotMods: number }
  | { Macro: number }
  | { Custom: number }

export type Action = 'Transparent' | { Basic: Basic } | { HoldTap: { hold: Basic; tap: Basic; profile: number } }

export type HoldTapFlavor = 'TapPreferred' | 'Balanced' | 'HoldPreferred'
export interface HoldTapProfile {
  flavor: HoldTapFlavor
  tapping_term_ms: number
  require_prior_idle_ms: number
}

export interface Combo {
  keys: number[]
  action: Action
  timeout_ms: number
}

export type MacroStep = { Tap: number } | { Press: number } | { Release: number } | { DelayMs: number }

/** `Both`: two switches per crossing, their diodes pointing opposite ways (a duplex matrix). */
export type DiodeDirection = 'Col2Row' | 'Row2Col' | 'Both'
export type MatrixConfig =
  | 'None'
  | { Direct: { pins: Pin[]; active_high: boolean } }
  | { Matrix: { rows: Pin[]; cols: Pin[]; diodes: DiodeDirection } }

export interface EncoderConfig {
  a: Pin
  b: Pin
  steps_per_detent: number
}

/** `Wire` is a cable with one data wire, on which the halves take turns. */
export type LinkConfig = { Uart: { tx: Pin; rx: Pin } } | 'Ble' | { Wire: { pin: Pin } }
/** `offset` is where this half's positions start in the keymap: 0 for the first half. */
export type SplitConfig = 'None' | { Half: { link: LinkConfig; offset: number } }

export interface HardwareConfig {
  matrix: MatrixConfig
  /**
   * Further groups of switches on a board that mixes wiring styles; their
   * positions follow the first group's. The keyboard keeps them apart from
   * the rest (see client.ts), so firmware from before them still reads.
   */
  more?: MatrixConfig[]
  /** How the switches are read; kept apart by the keyboard like `more`. Missing means the usual. */
  scan?: ScanConfig
  reserved_pins: Pin[]
  debounce_ms: number
  encoders: EncoderConfig[]
  split: SplitConfig
}

/**
 * `inverted`: this board's own switch positions (from 0 on each half) that
 * read closed at rest and open while pressed, ascending. `settle_us`: wait
 * after each matrix line changes, 0 for the usual. `drive_idle_high`: hold
 * the lines not being read high, for rows that switch power to sensors.
 */
export interface ScanConfig {
  inverted: number[]
  settle_us: number
  drive_idle_high: boolean
}
export const NO_SCAN: ScanConfig = { inverted: [], settle_us: 0, drive_idle_high: false }
/** The longest settle time the firmware accepts. */
export const MAX_SETTLE_US = 200

export type ColorOrder = 'Grb' | 'Rgb'
export type Effect = 'Off' | 'Solid' | 'Breathing' | 'Rainbow' | 'Reactive'

/** `key` is the position the light sits under, or 255 for underglow. x and y are 0..255 across the keyboard's longer side. */
export interface Led {
  key: number
  x: number
  y: number
}
export const UNDERGLOW = 255

/** Colour as hue, saturation and brightness, each 0..255. */
export interface LightZone {
  effect: Effect
  hue: number
  sat: number
  val: number
  speed: number
}

export interface LightingConfig {
  pin: Pin | null
  order: ColorOrder
  /** Every colour is scaled by this over 255. */
  limit: number
  leds: Led[]
  keys: LightZone
  underglow: LightZone
}

/** Hundredths of a key unit; rotation in tenths of a degree. w = 0 marks a position without a key. */
export interface KeyGeometry {
  x: number
  y: number
  w: number
  h: number
  r: number
}

export interface Meta {
  name: string
  controller: string
}

export type Mcu = 'Rp2040' | 'Rp2350' | 'Nrf52840' | 'Simulated'

export interface Info {
  protocol: number
  firmware: [number, number, number]
  mcu: Mcu
  usable_pins: number
  max_keys: number
  max_layers: number
  uid: number[]
}

export interface SplitStatus {
  connected: boolean
  remote_positions: number
  /** This half is the one typing right now. */
  leading: boolean
  /** How this half's keymap compares with the other half's, while connected. */
  sync: SplitSync
  /** The other half's chip; all zeros until it has said. */
  peer_uid: number[]
  /** How often the keymap was saved with a change in it. */
  revision: number
  peer_revision: number
  /** Equal on two halves exactly when they hold the same keymap. */
  digest: number
  /** `digest` when the halves last agreed, if they ever did. */
  base: number | null
}

/**
 * Behind: this half is fetching the other's newer keymap. Ahead: the other
 * half is fetching this one's. Conflict: both changed while apart.
 */
export type SplitSync = 'Unknown' | 'Same' | 'Behind' | 'Ahead' | 'Conflict'

export type Contact = { Ground: Pin } | { Supply: Pin } | { Link: { driven: Pin; sensed: Pin } }

export type Request =
  | 'GetInfo'
  | { Reboot: { bootloader: boolean } }
  | 'GetHardware'
  | { SetHardware: HardwareConfig }
  | 'GetLayerCount'
  | { SetLayerCount: number }
  | { GetActions: { layer: number; pos: number } }
  | { SetActions: { layer: number; pos: number; actions: Action[] } }
  | 'GetHoldTapProfiles'
  | { SetHoldTapProfile: { index: number; profile: HoldTapProfile } }
  | 'GetCombos'
  | { SetCombos: Combo[] }
  | { GetMacro: { index: number; start: number } }
  | { SetMacro: { index: number; start: number; steps: MacroStep[] } }
  | 'GetMeta'
  | { SetMeta: Meta }
  | { GetLayout: { start: number } }
  | { SetLayout: { start: number; keys: KeyGeometry[] } }
  | 'GetSplitStatus'
  | 'Save'
  | 'FactoryReset'
  | { WatchKeys: boolean }
  | { StartProbe: Pin[] }
  | 'StopProbe'
  /** Hold a pin low for another board to find, or let go with null. */
  | { HoldLow: Pin | null }
  | 'GetLighting'
  | { SetLighting: LightingConfig }
  /** Light one light alone to find it, or null for the normal effects. */
  | { IdentifyLed: number | null }
  | 'GetLayerColors'
  /** A colour number per layer (see layers.ts); 0 for none chosen. */
  | { SetLayerColors: number[] }
  | { GetMacroName: { index: number } }
  | { SetMacroName: { index: number; name: string } }
  | 'GetLetterRows'
  | { SetLetterRows: number[][] }
  | 'GetMoreWiring'
  | { SetMoreWiring: MatrixConfig[] }
  | { ResolveSplit: { keep_this: boolean } }
  | { SplitAgreed: { digest: number; revision: number } }
  | 'GetScan'
  | { SetScan: ScanConfig }
  /** A request for the other half of a split, in the bytes it takes over USB; its answer comes as a `Peer` event. */
  | { Peer: number[] }

export type DeviceError =
  | 'Malformed'
  | 'OutOfRange'
  | { UnusablePin: Pin }
  | { ReservedPin: Pin }
  | { DuplicatePin: Pin }
  | 'TooManyKeys'
  | 'Full'
  | 'Unsupported'
  | 'Storage'
  | 'NotConnected'

export type Response =
  | 'Ok'
  | { Error: DeviceError }
  | { Info: Info }
  | { Hardware: HardwareConfig }
  | { LayerCount: number }
  | { Actions: Action[] }
  | { HoldTapProfiles: HoldTapProfile[] }
  | { Combos: Combo[] }
  | { Macro: { total: number; steps: MacroStep[] } }
  | { Meta: Meta }
  | { Layout: { total: number; keys: KeyGeometry[] } }
  | { SplitStatus: SplitStatus }
  | { Lighting: LightingConfig }
  | { LayerColors: number[] }
  | { MacroName: string }
  | { LetterRows: number[][] }
  | { MoreWiring: MatrixConfig[] }
  | { Scan: ScanConfig }

export type DeviceEvent =
  | { Key: { pos: number; pressed: boolean } }
  | { Probe: Contact[] }
  /** The pressed key at `pos` took its action from `layer`. */
  | { Resolved: { pos: number; layer: number } }
  /** Bitmask of the layers now active. */
  | { Layers: number }
  /** A message from the other half of a split, in the bytes it would send over USB. */
  | { Peer: number[] }
export type DeviceMessage = { Response: Response } | { Event: DeviceEvent }

/** What a virtual keyboard's computer would receive. */
export type HostEvent = { Keyboard: { mods: number; keys: number[] } } | { Consumer: number }

/** `Sensor`: a light-beam key powered unless `power` is driven high, its comparator driving `out` low while closed. */
export type Wire =
  | { Ground: Pin }
  | { Supply: Pin }
  | { Plain: [Pin, Pin] }
  | { Diode: { anode: Pin; cathode: Pin } }
  | { Sensor: { power: Pin; out: Pin } }
export interface Wiring {
  switches: Wire[]
  encoders: [Pin, Pin][]
}

export const LIMITS = {
  actionsPerChunk: 16,
  macroStepsPerChunk: 32,
  layoutKeysPerChunk: 16,
  macros: 16,
  macroSteps: 512,
  combos: 16,
  comboKeys: 4,
  holdTapProfiles: 4,
  encoders: 4,
  nameLength: 32,
  macroNameLength: 16,
  leds: 120,
}

export function describeError(error: DeviceError): string {
  if (typeof error === 'string') {
    return {
      Malformed: 'The keyboard could not read the request. The app and firmware versions may not match.',
      OutOfRange: 'A value was outside what the keyboard supports.',
      TooManyKeys: 'That is more keys than the firmware supports.',
      Full: 'The keyboard has no room left for this.',
      Unsupported: 'This chip cannot do that with these pins.',
      Storage: 'Saving to the keyboard failed.',
      NotConnected: 'The other half does not answer over the cable.',
    }[error]
  }
  if ('UnusablePin' in error) return `Pin ${error.UnusablePin} cannot be used on this chip.`
  if ('ReservedPin' in error) return `Pin ${error.ReservedPin} is marked as reserved.`
  return `Pin ${error.DuplicatePin} is used twice.`
}
