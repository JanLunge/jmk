// A connected keyboard and everything the app knows about it. Edits go to
// the keyboard at once (so they can be tried immediately) and are written
// to its flash when the user saves.

import { markRaw, reactive, type Raw } from 'vue'
import { KeyboardClient } from './client'
import type { DemoHalf } from './demos'
import { localPositions } from './definition'
import type { Definition, Keymap } from './definition'
import { DEFINITION_SCHEMA } from './definition'
import type {
  Action,
  Combo,
  HardwareConfig,
  HoldTapProfile,
  HostEvent,
  Info,
  KeyGeometry,
  MacroStep,
  Meta,
  SplitStatus,
} from './protocol'
import { LIMITS } from './protocol'
import { bounds, gridLayout, hasKey, NO_KEY, UNIT } from './layout'
import type { Transport } from './transport'
import { VirtualTransport } from './transport'

export interface Session {
  id: string
  client: Raw<KeyboardClient>
  transport: Raw<Transport>
  /** The clickable picture of a virtual keyboard. */
  demo?: DemoHalf
  info: Info
  hardware: HardwareConfig
  meta: Meta
  layout: KeyGeometry[]
  layers: Action[][]
  profiles: HoldTapProfile[]
  combos: Combo[]
  macros: MacroStep[][]
  split: SplitStatus
  /** Positions currently held down, as reported by the keyboard. */
  pressed: Set<number>
  /** Changes not yet written to the keyboard's flash. */
  unsaved: boolean
  /** What a virtual keyboard typed, newest last. */
  typed: HostEvent[]
  closed: boolean
  /** The setup wizard is showing for this keyboard. */
  setup: boolean
}

let counter = 0

/** Positions in the keymap: this half's own plus the other half's. */
export function positionCount(session: Session): number {
  const own = localPositions(session.hardware)
  const split = session.hardware.split
  return split !== 'None' && 'Central' in split ? own + session.split.remote_positions : own
}

export function isConfigured(session: Session): boolean {
  return session.hardware.matrix !== 'None'
}

export function role(session: Session): 'single' | 'central' | 'peripheral' {
  const split = session.hardware.split
  return split === 'None' ? 'single' : 'Central' in split ? 'central' : 'peripheral'
}

export async function openSession(transport: Transport, demo?: DemoHalf): Promise<Session> {
  const client = new KeyboardClient(transport)
  const info = await client.info()
  // The client and transport are plain machinery; Vue must not wrap them.
  const session = reactive<Session>({
    id: `kb${++counter}`,
    client: markRaw(client),
    transport: markRaw(transport),
    demo,
    info,
    hardware: await client.hardware(),
    meta: { name: '', controller: '' },
    layout: [],
    layers: [],
    profiles: [],
    combos: [],
    macros: [],
    split: { connected: false, remote_positions: 0 },
    pressed: new Set<number>(),
    unsaved: false,
    typed: [],
    closed: false,
    setup: false,
  }) as Session
  client.onKey = (pos, pressed) => {
    if (pressed) session.pressed.add(pos)
    else session.pressed.delete(pos)
  }
  transport.onClose = () => (session.closed = true)
  if (transport instanceof VirtualTransport) {
    transport.onHost = (event) => {
      session.typed.push(event)
      if (session.typed.length > 200) session.typed.splice(0, session.typed.length - 200)
    }
  }
  await refresh(session)
  await client.watchKeys(true)
  session.setup = !isConfigured(session)
  return session
}

/** Read the whole configuration back from the keyboard. */
export async function refresh(session: Session): Promise<void> {
  const { client } = session
  session.hardware = await client.hardware()
  session.split = await client.splitStatus()
  session.meta = await client.meta()
  session.layout = await client.layout()
  session.profiles = await client.profiles()
  session.combos = await client.combos()
  const layerCount = await client.layerCount()
  // Read as many positions as could be in use, so nothing is lost when the
  // other half is not attached right now.
  const positions = Math.max(positionCount(session), session.layout.length)
  const layers: Action[][] = []
  for (let layer = 0; layer < layerCount; layer++) layers.push(await client.actions(layer, positions))
  session.layers = layers
  const macros: MacroStep[][] = []
  for (let index = 0; index < LIMITS.macros; index++) macros.push(await client.macro(index))
  session.macros = macros
  session.pressed.clear()
}

/** Make sure every layer has an entry for every position. */
export async function fitLayers(session: Session): Promise<void> {
  const positions = Math.max(positionCount(session), session.layout.length)
  for (const [index, layer] of session.layers.entries()) {
    if (layer.length < positions) {
      const more = await session.client.actions(index, positions)
      layer.splice(0, layer.length, ...more)
    }
  }
}

export async function setHardware(session: Session, hardware: HardwareConfig): Promise<void> {
  await session.client.setHardware(hardware)
  session.hardware = hardware
  session.pressed.clear()
  session.unsaved = true
  await fitLayers(session)
}

export async function setAction(session: Session, layer: number, pos: number, action: Action): Promise<void> {
  await session.client.setActions(layer, pos, [action])
  session.layers[layer][pos] = action
  session.unsaved = true
}

export async function setLayerCount(session: Session, count: number): Promise<void> {
  await session.client.setLayerCount(count)
  const positions = Math.max(positionCount(session), session.layout.length)
  while (session.layers.length > count) session.layers.pop()
  while (session.layers.length < count) {
    const index = session.layers.length
    // A new layer starts see-through, whatever was stored there before.
    const blank: Action[] = Array(positions).fill('Transparent')
    await session.client.setActions(index, 0, blank)
    session.layers.push(blank)
  }
  session.unsaved = true
}

export async function setLayout(session: Session, layout: KeyGeometry[]): Promise<void> {
  await session.client.setLayout(layout)
  session.layout = layout
  session.unsaved = true
  await fitLayers(session)
}

export async function setMeta(session: Session, meta: Meta): Promise<void> {
  await session.client.setMeta(meta)
  session.meta = meta
  session.unsaved = true
}

export async function setProfile(session: Session, index: number, profile: HoldTapProfile): Promise<void> {
  await session.client.setProfile(index, profile)
  session.profiles[index] = profile
  session.unsaved = true
}

export async function setCombos(session: Session, combos: Combo[]): Promise<void> {
  await session.client.setCombos(combos)
  session.combos = combos
  session.unsaved = true
}

export async function setMacro(session: Session, index: number, steps: MacroStep[]): Promise<void> {
  await session.client.setMacro(index, steps)
  session.macros[index] = steps
  session.unsaved = true
}

export async function save(session: Session): Promise<void> {
  await session.client.save()
  session.unsaved = false
}

export function exportKeymap(session: Session): Keymap {
  return JSON.parse(
    JSON.stringify({ layers: session.layers, profiles: session.profiles, combos: session.combos, macros: session.macros }),
  )
}

/** Write a keymap to the keyboard, replacing the current one. */
export async function applyKeymap(session: Session, keymap: Keymap): Promise<void> {
  const { client } = session
  await client.setLayerCount(Math.max(1, keymap.layers.length))
  for (const [index, layer] of keymap.layers.entries()) await client.setActions(index, 0, layer)
  for (const [index, profile] of keymap.profiles.slice(0, LIMITS.holdTapProfiles).entries()) await client.setProfile(index, profile)
  await client.setCombos(keymap.combos.slice(0, LIMITS.combos))
  for (let index = 0; index < LIMITS.macros; index++) await client.setMacro(index, keymap.macros[index] ?? [])
  session.unsaved = true
}

/**
 * The keyboard as a shareable file. For a split, pass the other half's
 * hardware so the file can set up both.
 */
export function exportDefinition(session: Session, otherHalf?: HardwareConfig): Definition {
  const halves = [{ hardware: session.hardware }]
  if (otherHalf) halves.push({ hardware: otherHalf })
  return JSON.parse(
    JSON.stringify({
      schema: DEFINITION_SCHEMA,
      name: session.meta.name || 'My keyboard',
      controller: session.meta.controller,
      mcu: session.info.mcu,
      halves,
      layout: session.layout,
      keymap: exportKeymap(session),
    }),
  )
}

/** Configure this board as half number `half` of a definition. */
export async function applyDefinition(session: Session, definition: Definition, half = 0): Promise<void> {
  const hardware = definition.halves[half]?.hardware
  if (!hardware) throw new Error('The keyboard file has no such half.')
  await session.client.setHardware(hardware)
  await session.client.setMeta({
    name: definition.name.slice(0, LIMITS.nameLength),
    controller: definition.controller.slice(0, LIMITS.nameLength),
  })
  if (half === 0) {
    await session.client.setLayout(definition.layout)
    await applyKeymap(session, definition.keymap)
  }
  await session.client.save()
  await refresh(session)
  session.unsaved = false
}

/** Letters, then digits: a first keymap that shows every key working. */
export function starterAction(index: number): Action {
  const usages = [...Array(26).keys()].map((i) => 0x04 + i).concat([...Array(10).keys()].map((i) => 0x1e + i))
  return index < usages.length ? { Basic: { Key: usages[index] } } : { Basic: 'None' }
}

/** A grid for the switches plus an empty entry per knob direction. */
export function starterLayout(hardware: HardwareConfig, populated?: number[]): KeyGeometry[] {
  return [...gridLayout(hardware.matrix, populated), ...Array(2 * hardware.encoders.length).fill(NO_KEY)]
}

/**
 * Central half: when the other half reports more positions than the layout
 * has keys for, draw them to the right and give them starter actions.
 * `partner` is the other half's session, if it is open, for its key shapes.
 */
export async function adoptRemoteKeys(session: Session, partner?: Session): Promise<boolean> {
  const own = localPositions(session.hardware)
  const total = positionCount(session)
  if (role(session) !== 'central' || session.layout.length >= total) return false
  const layout = session.layout.slice(0, Math.max(session.layout.length, own))
  while (layout.length < own) layout.push({ ...NO_KEY })
  const missing = total - layout.length
  const box = bounds(layout)
  const shiftX = Math.round(box.x + box.w + UNIT)
  const theirs = partner?.layout.length === missing ? partner.layout : gridLayout({ Direct: { pins: Array(missing).fill(0), active_high: false } })
  const first = layout.length
  layout.push(...theirs.map((key) => ({ ...key, x: key.w > 0 ? key.x + shiftX : key.x })))
  await session.client.setLayout(layout)
  session.layout = layout
  await fitLayers(session)
  let letter = session.layers[0].slice(0, first).filter((a) => a !== 'Transparent').length
  const actions = theirs.map((key) => (hasKey(key) ? starterAction(letter++) : 'Transparent'))
  await session.client.setActions(0, first, actions)
  session.layers[0].splice(first, actions.length, ...actions)
  session.unsaved = true
  return true
}
