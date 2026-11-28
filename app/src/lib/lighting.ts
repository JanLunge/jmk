// Helpers for the RGB lights: where a light sits in the picture and how the
// firmware's colour numbers map to what a screen shows.

import { bounds, hasKey } from './layout'
import type { ColorOrder, KeyGeometry, Led, LightZone } from './protocol'
import { UNDERGLOW } from './protocol'

const span = (layout: KeyGeometry[]) => {
  const box = bounds(layout)
  return { box, size: Math.max(box.w, box.h, 1) }
}

/** The firmware's coordinates (0..255 across the longer side) for the middle of a key. */
export function placeAtKey(layout: KeyGeometry[], pos: number): { x: number; y: number } {
  const key = layout[pos]
  const { box, size } = span(layout)
  if (!hasKey(key)) return { x: 0, y: 0 }
  const scale = (value: number) => Math.max(0, Math.min(255, Math.round((value / size) * 255)))
  return { x: scale(key.x + key.w / 2 - box.x), y: scale(key.y + key.h / 2 - box.y) }
}

/** Where a light is drawn, in the layout's own units. */
export function drawnAt(layout: KeyGeometry[], led: Led): { x: number; y: number } {
  const { box, size } = span(layout)
  return { x: box.x + (led.x / 255) * size, y: box.y + (led.y / 255) * size }
}

/**
 * Underglow as it is usually built: the lights in a ring under the edge of
 * the keyboard, in chain order clockwise from the top left corner.
 */
export function ring(layout: KeyGeometry[], count: number): Led[] {
  const { box, size } = span(layout)
  const w = (box.w / size) * 255
  const h = (box.h / size) * 255
  const around = 2 * (w + h)
  return Array.from({ length: count }, (_, index) => {
    // Half a step in, so no light sits exactly on a corner.
    let along = ((index + 0.5) / count) * around
    let at: [number, number]
    if (along < w) at = [along, 0]
    else if ((along -= w) < h) at = [w, along]
    else if ((along -= h) < w) at = [w - along, h]
    else at = [0, h - (along - w)]
    return { key: UNDERGLOW, x: Math.round(at[0]), y: Math.round(at[1]) }
  })
}

/** One light under each key, in keymap order, as a start for placing them by hand. `skip` lists positions without a light (knobs). */
export function underKeys(layout: KeyGeometry[], count: number, skip: number[] = []): Led[] {
  const keys = layout.map((key, pos) => ({ key, pos })).filter(({ key, pos }) => hasKey(key) && !skip.includes(pos))
  if (!keys.length) return ring(layout, count)
  return Array.from({ length: count }, (_, index) => {
    const pos = keys[index % keys.length].pos
    return { key: pos, ...placeAtKey(layout, pos) }
  })
}

export type LightMode = 'underglow' | 'keys' | 'both'

export function lightMode(leds: Led[]): LightMode {
  const under = leds.filter((led) => led.key === UNDERGLOW).length
  return under === leds.length ? 'underglow' : under === 0 ? 'keys' : 'both'
}

/** The firmware's hue/saturation/brightness (0..255 each) as red, green, blue. Mirrors crates/jmk-device/src/lights.rs. */
export function hsvToRgb(hue: number, sat: number, val: number): [number, number, number] {
  if (sat === 0) return [val, val, val]
  const region = Math.floor(hue / 43)
  const rest = (hue - region * 43) * 6
  const p = Math.floor((val * (255 - sat)) / 255)
  const q = Math.floor((val * (255 - Math.floor((sat * rest) / 255))) / 255)
  const t = Math.floor((val * (255 - Math.floor((sat * (255 - rest)) / 255))) / 255)
  return ([[val, t, p], [q, val, p], [p, val, t], [p, q, val], [t, p, val], [val, p, q]] as [number, number, number][])[Math.min(region, 5)]
}

export function rgbToHsv(r: number, g: number, b: number): { hue: number; sat: number; val: number } {
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const delta = max - min
  let hue = 0
  if (delta > 0) {
    if (max === r) hue = ((g - b) / delta + 6) % 6
    else if (max === g) hue = (b - r) / delta + 2
    else hue = (r - g) / delta + 4
  }
  return { hue: Math.round((hue / 6) * 255) % 256, sat: max === 0 ? 0 : Math.round((delta / max) * 255), val: max }
}

const hex = (rgb: number[]) => '#' + rgb.map((part) => Math.round(part).toString(16).padStart(2, '0')).join('')

/** A zone's colour at full brightness, for a colour picker. */
export const zoneHex = (zone: { hue: number; sat: number }) => hex(hsvToRgb(zone.hue, zone.sat, 255))

export function hexToHueSat(value: string): { hue: number; sat: number } {
  const parts = [1, 3, 5].map((at) => parseInt(value.slice(at, at + 2), 16))
  const { hue, sat } = rgbToHsv(parts[0], parts[1], parts[2])
  return { hue, sat }
}

/**
 * How a light looks on screen. A screen cannot be as dim as a light can, so
 * low levels are lifted: an LED at a tenth of its power is still plainly lit.
 */
export function shown(rgb: number[]): string {
  const peak = Math.max(...rgb)
  if (peak === 0) return '#000000'
  const lift = Math.sqrt(peak / 255) / (peak / 255)
  return hex(rgb.map((part) => Math.min(255, part * lift)))
}

/** Bytes as sent down the chain, back to red, green, blue. */
export const fromWire = (order: ColorOrder, bytes: number[]): number[] => (order === 'Grb' ? [bytes[1], bytes[0], bytes[2]] : bytes)

/** What a zone shows when the app cannot see the lights themselves. */
export function zonePreview(zone: LightZone, limit: number): number[] {
  if (zone.effect === 'Off') return [0, 0, 0]
  return hsvToRgb(zone.hue, zone.effect === 'Rainbow' ? Math.max(zone.sat, 1) : zone.sat, zone.val).map((part) => Math.floor((part * limit) / 255))
}
