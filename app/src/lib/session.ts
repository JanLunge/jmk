// A connected keyboard and everything the app knows about it. Edits go to
// the keyboard at once (so they can be tried immediately) and are written
// to its flash when the user saves.

import { markRaw, reactive, type Raw } from 'vue'
import type { AddPlan } from './addkey'
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
  Pin,
  KeyGeometry,
  LightingConfig,
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
  /** A colour number per layer; 0 where none was chosen. */
  layerColors: number[]
  profiles: HoldTapProfile[]
  combos: Combo[]
  macros: MacroStep[][]
  split: SplitStatus
  lighting: LightingConfig
  /** The light pin the firmware started with; a change needs a restart. */
  bootLightPin: Pin | null
  /** A virtual keyboard's lights as they shine right now, in chain order. */
  lights: number[][]
  /** Positions currently held down, as reported by the keyboard. */
  pressed: Set<number>
  /**
   * The firmware reports which layers are active and which layer each press
   * used. False on firmware from before it did; then only `pressed` is known.
   */
  layerAware: boolean
  /** Bitmask of the layers active on the keyboard right now. */
  activeLayers: number
  /** For held keys the keyboard has acted on: the layer the action came from. */
  usedLayer: Map<number, number>
  /** The cable pins found for this half by looking for the wires (see cable.ts). */
  cable?: { tx: Pin; rx: Pin }
  /** The cable setting the firmware started with; a change needs a restart. */
  bootLink: string
  /** When the keyboard last reported a key going down or up (performance.now()). */
  lastKeyAt: number
  /** The other half changed this half's keymap; read it again before showing it. */
  stale?: boolean
  /** Changes not yet written to the keyboard's flash. */
  unsaved: boolean
  /** What a virtual keyboard typed, newest last. */
  typed: HostEvent[]
  closed: boolean
  /** The setup wizard is showing for this keyboard. */
  setup: boolean
  /** It was told to restart or update; it will drop off and come back. */
  restarting?: boolean
}

let counter = 0

/** Protocol version from which the firmware runs RGB lights. */
export const LIGHTING_PROTOCOL = 3
export const canLight = (session: Session) => session.info.protocol >= LIGHTING_PROTOCOL

export function noLighting(): LightingConfig {
  const off = { effect: 'Off' as const, hue: 0, sat: 0, val: 0, speed: 0 }
  return { pin: null, order: 'Grb', limit: 80, leds: [], keys: { ...off }, underglow: { ...off } }
}

/** Protocol version from which the firmware remembers a colour per layer. */
export const LAYER_COLOR_PROTOCOL = 4
export const canColorLayers = (session: Session) => session.info.protocol >= LAYER_COLOR_PROTOCOL

// Settings that are replaced as a whole (the lights, the combos, a macro)
// change in the app at once, so the next click builds on this one, and go to
// the keyboard one after the other. Of several waiting only the newest is sent.
const outboxes = new WeakMap<Session, Map<string, { busy: boolean; next?: () => Promise<void> }>>()

async function sendNewest(session: Session, what: string, send: () => Promise<void>): Promise<void> {
  session.unsaved = true
  const boxes = outboxes.get(session) ?? new Map<string, { busy: boolean; next?: () => Promise<void> }>()
  outboxes.set(session, boxes)
  const box = boxes.get(what) ?? { busy: false }
  boxes.set(what, box)
  if (box.busy) {
    box.next = send
    return
  }
  box.busy = true
  try {
    for (let now: (() => Promise<void>) | undefined = send; now; ) {
      await now()
      now = box.next
      box.next = undefined
    }
  } catch (error) {
    box.next = undefined
    box.busy = false
    // The keyboard refused: show what it really has.
    await refresh(session).catch(() => {})
    throw error
  } finally {
    box.busy = false
  }
}

/** Change the lights. Shows at once; the data pin takes effect after a save and restart. */
export function setLighting(session: Session, lighting: LightingConfig): Promise<void> {
  session.lighting = lighting
  return sendNewest(session, 'lighting', () => session.client.setLighting(lighting))
}

/** Give a layer a colour number (see layers.ts). Firmware from before layer colours keeps it only until the app closes. */
export function setLayerColor(session: Session, layer: number, color: number): Promise<void> {
  const colors = [...Array(session.layers.length).keys()].map((index) => (index === layer ? color : (session.layerColors[index] ?? 0)))
  session.layerColors = colors
  if (!canColorLayers(session)) return Promise.resolve()
  return sendNewest(session, 'layerColors', () => session.client.setLayerColors(colors))
}

/** Protocol version from which the halves of a split keep their keymaps the same over their cable. */
export const SPLIT_PROTOCOL = 6

const NO_SPLIT: SplitStatus = { connected: false, remote_positions: 0, leading: true,
  sync: 'Unknown',
  peer_uid: [0, 0, 0, 0, 0, 0, 0, 0],
  revision: 0,
  peer_revision: 0,
  digest: 0,
  base: null,
}

/** The cable pins a half is set up with. */
export function linkPins(hardware: HardwareConfig): { tx: Pin; rx: Pin } | undefined {
  const split = hardware.split
  if (split === 'None' || split.Half.link === 'Ble') return undefined
  return split.Half.link.Uart
}

/** Where this board's positions start in the keymap. */
export const splitOffset = (hardware: HardwareConfig) => (hardware.split === 'None' ? 0 : hardware.split.Half.offset)

/** Positions in the keymap: this half's own plus the other half's. */
export function positionCount(session: Session): number {
  const own = localPositions(session.hardware)
  if (session.hardware.split === 'None') return own
  const offset = splitOffset(session.hardware)
  return offset > 0 ? offset + own : own + session.split.remote_positions
}

export function isConfigured(session: Session): boolean {
  return session.hardware.matrix !== 'None'
}

/** One piece, or which half of a split: the first half's keys come first in the keymap. */
export function role(session: Session): 'single' | 'first' | 'second' {
  if (session.hardware.split === 'None') return 'single'
  return splitOffset(session.hardware) === 0 ? 'first' : 'second'
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
    layerColors: [],
    profiles: [],
    combos: [],
    macros: [],
    split: { ...NO_SPLIT },
    lighting: noLighting(),
    bootLightPin: null,
    lights: [],
    pressed: new Set<number>(),
    layerAware: false,
    activeLayers: 1,
    usedLayer: new Map<number, number>(),
    bootLink: '',
    lastKeyAt: -Infinity,
    unsaved: false,
    typed: [],
    closed: false,
    setup: false,
  }) as Session
  client.onKey = (pos, pressed) => {
    session.lastKeyAt = performance.now()
    if (pressed) session.pressed.add(pos)
    else {
      session.pressed.delete(pos)
      session.usedLayer.delete(pos)
    }
  }
  client.onResolved = (pos, layer) => {
    // A tap can be decided only after its release; that is no longer held.
    if (session.pressed.has(pos)) session.usedLayer.set(pos, layer)
  }
  client.onLayers = (layers) => {
    session.layerAware = true
    session.activeLayers = layers
  }
  transport.onClose = () => (session.closed = true)
  if (transport instanceof VirtualTransport) {
    transport.onLights = (colors) => (session.lights = colors)
    transport.onHost = (event) => {
      session.typed.push(event)
      if (session.typed.length > 200) session.typed.splice(0, session.typed.length - 200)
    }
  }
  await refresh(session)
  session.bootLink = linkOf(session.hardware)
  session.bootLightPin = session.lighting.pin
  await client.watchKeys(true)
  session.setup = !isConfigured(session)
  if (isConfigured(session) && session.layout.length === 0) {
    // Set up by other means (command line, older app): draw a grid so the
    // keys can be seen and arranged.
    await setLayout(session, starterLayout(session.hardware))
  }
  return session
}

/** Read the whole configuration back from the keyboard. */
export async function refresh(session: Session): Promise<void> {
  const { client } = session
  session.hardware = await client.hardware()
  session.split = session.info.protocol >= SPLIT_PROTOCOL ? await client.splitStatus() : { ...NO_SPLIT }
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
  session.lighting = canLight(session) ? await client.lighting() : noLighting()
  if (canColorLayers(session)) session.layerColors = await client.layerColors()
  session.pressed.clear()
  session.usedLayer.clear()
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
  session.usedLayer.clear()
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

export function setCombos(session: Session, combos: Combo[]): Promise<void> {
  session.combos = combos
  return sendNewest(session, 'combos', () => session.client.setCombos(combos))
}

export function setMacro(session: Session, index: number, steps: MacroStep[]): Promise<void> {
  session.macros[index] = steps
  return sendNewest(session, 'macro' + index, () => session.client.setMacro(index, steps))
}

/**
 * Add the key a plan describes: extend the wiring if needed, draw the key
 * and give it a starter letter. Everything already set up keeps its key,
 * even where the positions are renumbered. Returns the key's position.
 */
export async function addKey(session: Session, plan: Extract<AddPlan, { ok: true }>): Promise<number> {
  const { client } = session
  const oldLength = Math.max(positionCount(session), session.layout.length, ...session.layers.map((l) => l.length))
  const length = Math.max(plan.pos, ...[...Array(oldLength).keys()].map(plan.move)) + 1
  const layout: KeyGeometry[] = Array.from({ length }, () => ({ ...NO_KEY }))
  session.layout.forEach((key, old) => (layout[plan.move(old)] = key))
  if (hasKey(layout[plan.pos])) throw new Error('That key is already part of the keyboard.')
  const letters = layout.filter(hasKey).length
  layout[plan.pos] = freeSpot(session.layout, plan)
  const layers = session.layers.map((layer) => {
    const moved: Action[] = Array(length).fill('Transparent')
    layer.forEach((action, old) => (moved[plan.move(old)] = action))
    return moved
  })
  const before = layers[0][plan.pos]
  if (before === 'Transparent' || JSON.stringify(before) === JSON.stringify({ Basic: 'None' })) {
    layers[0][plan.pos] = starterAction(letters)
  }
  const combos = session.combos.map((combo) => ({ ...combo, keys: combo.keys.map(plan.move) }))
  try {
    if (plan.rewired) await client.setHardware(plan.hardware)
    await client.setLayout(layout)
    for (const [index, layer] of layers.entries()) await client.setActions(index, 0, layer)
    if (plan.rewired && combos.length) await client.setCombos(combos)
  } catch (error) {
    // Half applied: show what the keyboard really has now.
    await refresh(session).catch(() => {})
    throw error
  }
  session.hardware = plan.hardware
  session.layout = layout
  session.layers = layers
  session.combos = combos
  session.pressed.clear()
  session.usedLayer.clear()
  session.unsaved = true
  return plan.pos
}

/** A place to draw an added key: its spot in the grid if that is empty, else under the keyboard. */
function freeSpot(layout: KeyGeometry[], plan: Extract<AddPlan, { ok: true }>): KeyGeometry {
  const matrix = plan.hardware.matrix
  const drawn = layout.filter(hasKey)
  if (matrix !== 'None' && 'Matrix' in matrix) {
    const cols = matrix.Matrix.cols.length
    const spot = { x: (plan.pos % cols) * UNIT, y: Math.floor(plan.pos / cols) * UNIT, w: UNIT, h: UNIT, r: 0 }
    const overlaps = (key: KeyGeometry) =>
      key.x < spot.x + spot.w && spot.x < key.x + key.w && key.y < spot.y + spot.h && spot.y < key.y + key.h
    if (!drawn.some(overlaps)) return spot
  }
  const box = bounds(drawn)
  return { x: Math.round(box.x), y: Math.round(box.y + box.h + UNIT / 4), w: UNIT, h: UNIT, r: 0 }
}

export async function save(session: Session): Promise<void> {
  await session.client.save()
  session.unsaved = false
}

const linkOf = (hardware: HardwareConfig) => JSON.stringify(hardware.split === 'None' ? null : hardware.split.Half.link)

/** The cable setting was changed and saved, but the firmware still runs the one it started with. */
export function needsRestart(session: Session): boolean {
  return !session.unsaved && (linkOf(session.hardware) !== session.bootLink || session.lighting.pin !== session.bootLightPin)
}

/**
 * Restart the keyboard so a saved cable setting takes effect. A USB
 * keyboard drops off and comes back as a fresh session by itself.
 */
export async function restart(session: Session): Promise<void> {
  session.restarting = !(session.transport instanceof VirtualTransport)
  await session.client.reboot(false)
  if (session.transport instanceof VirtualTransport) {
    await refresh(session)
    session.bootLink = linkOf(session.hardware)
    session.bootLightPin = session.lighting.pin
    await session.client.watchKeys(true)
  }
}

export function exportKeymap(session: Session): Keymap {
  return JSON.parse(
    JSON.stringify({
      layers: session.layers,
      profiles: session.profiles,
      combos: session.combos,
      macros: session.macros,
      layerColors: session.layerColors.slice(0, session.layers.length),
    }),
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
  if (canColorLayers(session)) await client.setLayerColors((keymap.layerColors ?? []).slice(0, session.info.max_layers))
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
      lighting: session.lighting.pin === null ? undefined : session.lighting,
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
  // Both halves of a split hold the whole keymap; the lights are the first half's.
  await session.client.setLayout(definition.layout)
  await applyKeymap(session, definition.keymap)
  if (half === 0 && definition.lighting && canLight(session)) await session.client.setLighting(definition.lighting)
  await session.client.save()
  await refresh(session)
  session.unsaved = false
}

/** Letters, then digits: a first keymap that shows every key working. */
export function starterAction(index: number): Action {
  const usages = [...Array(26).keys()].map((i) => 0x04 + i).concat([...Array(10).keys()].map((i) => 0x1e + i))
  return index < usages.length ? { Basic: { Key: usages[index] } } : { Basic: 'None' }
}

/** A grid for the switches, then per knob its place in the picture and an empty entry. */
export function starterLayout(hardware: HardwareConfig, populated?: number[]): KeyGeometry[] {
  const layout = gridLayout(hardware.matrix, populated)
  hardware.encoders.forEach(() => layout.push(knobSpot(layout), { ...NO_KEY }))
  return layout
}

/** Key positions that are a knob's picture: the first of its two positions. */
export function knobPositions(session: Session): number[] {
  const count = session.hardware.encoders.length
  const first = splitOffset(session.hardware) + localPositions(session.hardware) - 2 * count
  return [...Array(count).keys()].map((index) => first + 2 * index)
}

/** A free place for a knob: above the keys, after the knobs already there. */
export function knobSpot(layout: KeyGeometry[]): KeyGeometry {
  const box = bounds(layout)
  const y = Math.round(box.y - UNIT * 1.25)
  const taken = layout.filter((key) => hasKey(key) && key.y === y).length
  return { x: Math.round(box.x + taken * UNIT * 1.25), y, w: UNIT, h: UNIT, r: 0 }
}

/** What both halves of a split must agree on, as text to compare. */
function sharedPart(session: Session): string {
  const keymap = exportKeymap(session)
  // No colour chosen and colour 0 are the same thing.
  const part = [session.layout, { ...keymap, layerColors: keymap.layers.map((_, layer) => keymap.layerColors?.[layer] ?? 0) }]
  // What was read from a keyboard and what was made here list their fields in different orders.
  const sorted = (_: string, value: unknown) =>
    value && typeof value === 'object' && !Array.isArray(value)
      ? Object.fromEntries(Object.entries(value).sort(([a], [b]) => (a < b ? -1 : 1)))
      : value
  return JSON.stringify(part, sorted)
}

/**
 * Make `to` hold the same keymap and key shapes as `from`, and make the
 * second half's positions start where the first half's end. Does nothing
 * where the two agree already. Returns whether anything was written.
 */
export async function alignHalves(from: Session, to: Session): Promise<boolean> {
  // Writing to one half must not echo back to the other while this runs.
  const mirrors = [from.client.mirror, to.client.mirror]
  from.client.mirror = to.client.mirror = undefined
  try {
    if (to.stale) await refresh(to)
    to.stale = false
    let changed = false
    const [first, second] = splitOffset(from.hardware) <= splitOffset(to.hardware) ? [from, to] : [to, from]
    const offset = localPositions(first.hardware)
    const split = second.hardware.split
    if (split !== 'None' && split.Half.offset !== offset) {
      await setHardware(second, { ...second.hardware, split: { Half: { ...split.Half, offset } } })
      changed = true
    }
    if (sharedPart(from) !== sharedPart(to)) {
      await to.client.setLayout(from.layout)
      await applyKeymap(to, exportKeymap(from))
      await refresh(to)
      to.unsaved = true
      changed = true
    }
    if (from.meta.name !== to.meta.name) {
      await setMeta(to, { ...to.meta, name: from.meta.name })
      changed = true
    }
    return changed
  } finally {
    ;[from.client.mirror, to.client.mirror] = mirrors
  }
}
