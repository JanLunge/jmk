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

export type DiodeDirection = 'Col2Row' | 'Row2Col'
export type MatrixConfig =
  | 'None'
  | { Direct: { pins: Pin[]; active_high: boolean } }
  | { Matrix: { rows: Pin[]; cols: Pin[]; diodes: DiodeDirection } }

export interface EncoderConfig {
  a: Pin
  b: Pin
  steps_per_detent: number
}

export type LinkConfig = { Uart: { tx: Pin; rx: Pin } } | 'Ble'
export type SplitConfig = 'None' | { Central: LinkConfig } | { Peripheral: LinkConfig }

export interface HardwareConfig {
  matrix: MatrixConfig
  reserved_pins: Pin[]
  debounce_ms: number
  encoders: EncoderConfig[]
  split: SplitConfig
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
}

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

export type DeviceEvent = { Key: { pos: number; pressed: boolean } } | { Probe: Contact[] }
export type DeviceMessage = { Response: Response } | { Event: DeviceEvent }

/** What a virtual keyboard's computer would receive. */
export type HostEvent = { Keyboard: { mods: number; keys: number[] } } | { Consumer: number }

export type Wire = { Ground: Pin } | { Supply: Pin } | { Plain: [Pin, Pin] } | { Diode: { anode: Pin; cathode: Pin } }
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
    }[error]
  }
  if ('UnusablePin' in error) return `Pin ${error.UnusablePin} cannot be used on this chip.`
  if ('ReservedPin' in error) return `Pin ${error.ReservedPin} is marked as reserved.`
  return `Pin ${error.DuplicatePin} is used twice.`
}
