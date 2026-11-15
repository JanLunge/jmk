// Key shapes for drawing the keyboard, and ways to produce them.

import type { KeyGeometry, MatrixConfig } from './protocol'

export const UNIT = 100
export const NO_KEY: KeyGeometry = { x: 0, y: 0, w: 0, h: 0, r: 0 }

export const hasKey = (key: KeyGeometry | undefined): key is KeyGeometry => !!key && key.w > 0

/** A plain grid, used until the user arranges the keys. `populated` lists the positions that have a switch. */
export function gridLayout(matrix: MatrixConfig, populated?: number[]): KeyGeometry[] {
  const count = matrix === 'None' ? 0 : 'Direct' in matrix ? matrix.Direct.pins.length : matrix.Matrix.rows.length * matrix.Matrix.cols.length
  const perRow = matrix !== 'None' && 'Matrix' in matrix ? matrix.Matrix.cols.length : Math.min(count, Math.max(1, Math.ceil(Math.sqrt(count * 2))))
  const present = populated ? new Set(populated) : undefined
  return Array.from({ length: count }, (_, pos) =>
    present && !present.has(pos)
      ? { ...NO_KEY }
      : { x: (pos % perRow) * UNIT, y: Math.floor(pos / perRow) * UNIT, w: UNIT, h: UNIT, r: 0 },
  )
}

export interface Bounds {
  x: number
  y: number
  w: number
  h: number
}

/** Corners of a key after rotation around its centre. */
export function corners(key: KeyGeometry): [number, number][] {
  const cx = key.x + key.w / 2
  const cy = key.y + key.h / 2
  const angle = (key.r / 10) * (Math.PI / 180)
  const cos = Math.cos(angle)
  const sin = Math.sin(angle)
  return [[-1, -1], [1, -1], [1, 1], [-1, 1]].map(([sx, sy]) => {
    const dx = (sx * key.w) / 2
    const dy = (sy * key.h) / 2
    return [cx + dx * cos - dy * sin, cy + dx * sin + dy * cos]
  })
}

export function bounds(keys: KeyGeometry[]): Bounds {
  const points = keys.filter(hasKey).flatMap(corners)
  if (!points.length) return { x: 0, y: 0, w: UNIT, h: UNIT }
  const xs = points.map((p) => p[0])
  const ys = points.map((p) => p[1])
  const x = Math.min(...xs)
  const y = Math.min(...ys)
  return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y }
}

/**
 * Read a layout in the format of keyboard-layout-editor.com ("raw data" or
 * downloaded JSON). Only position, size and rotation are used.
 */
export function parseKle(text: string): KeyGeometry[] {
  let rows: unknown
  const trimmed = text.trim()
  try {
    rows = JSON.parse(trimmed)
  } catch {
    // The site's raw data omits the outer brackets and quotes around names.
    rows = JSON.parse(`[${trimmed.replace(/([{,])\s*([a-z][a-z0-9]*)\s*:/gi, '$1"$2":')}]`)
  }
  if (!Array.isArray(rows)) throw new Error('This does not look like a keyboard-layout-editor layout.')
  const keys: KeyGeometry[] = []
  let y = 0
  let rotation = 0
  let originX = 0
  let originY = 0
  for (const row of rows) {
    if (!Array.isArray(row)) continue // the metadata object
    let x = originX
    let w = 1
    let h = 1
    for (const item of row) {
      if (typeof item === 'string') {
        keys.push(rotated({ x, y, w, h }, rotation, originX, originY))
        x += w
        w = 1
        h = 1
        continue
      }
      if (item.r !== undefined) rotation = item.r
      if (item.rx !== undefined) {
        originX = item.rx
        x = originX
        y = originY
      }
      if (item.ry !== undefined) {
        originY = item.ry
        y = originY
      }
      x += item.x ?? 0
      y += item.y ?? 0
      w = item.w ?? w
      h = item.h ?? h
    }
    y += 1
  }
  if (!keys.length) throw new Error('The layout contains no keys.')
  return keys
}

/** Turn a KLE key (rotated around a shared origin) into ours (rotated around its own centre). */
function rotated(key: { x: number; y: number; w: number; h: number }, degrees: number, ox: number, oy: number): KeyGeometry {
  const angle = (degrees * Math.PI) / 180
  const cx = key.x + key.w / 2 - ox
  const cy = key.y + key.h / 2 - oy
  const nx = ox + cx * Math.cos(angle) - cy * Math.sin(angle)
  const ny = oy + cx * Math.sin(angle) + cy * Math.cos(angle)
  return {
    x: Math.round((nx - key.w / 2) * UNIT),
    y: Math.round((ny - key.h / 2) * UNIT),
    w: Math.round(key.w * UNIT),
    h: Math.round(key.h * UNIT),
    r: Math.round(degrees * 10),
  }
}

/** Move the layout so its top-left corner sits at the origin. */
export function normalised(keys: KeyGeometry[]): KeyGeometry[] {
  const box = bounds(keys)
  return keys.map((key) => (hasKey(key) ? { ...key, x: Math.round(key.x - box.x), y: Math.round(key.y - box.y) } : key))
}
