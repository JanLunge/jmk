// The whole keymap as one picture, for printing and sharing. It is built as
// a self-contained SVG (every colour written out), so it looks the same in
// the app, in a file and on paper.

import { describeAction, legend, type Legend } from './keycodes'
import { layerHex } from './layers'
import { bounds, hasKey } from './layout'
import type { Action, Combo, KeyGeometry } from './protocol'

export interface PosterSource {
  name: string
  layout: KeyGeometry[]
  layers: Action[][]
  layerColors: number[]
  /** Positions drawn as a knob: the first of each knob's two. */
  knobs: number[]
  combos: Combo[]
}

export interface PosterOptions {
  /** Every layer as its own keyboard, or all layers printed on one. */
  mode: 'grid' | 'stacked'
  dark: boolean
}

export interface Poster {
  svg: string
  width: number
  height: number
}

interface Theme {
  bg: string
  panel: string
  cap: string
  edge: string
  text: string
  muted: string
}
const LIGHT: Theme = { bg: '#ffffff', panel: '#f6f4ef', cap: '#ffffff', edge: '#cfcabd', text: '#1d2026', muted: '#737989' }
const DARK: Theme = { bg: '#171717', panel: '#1f1f1f', cap: '#333333', edge: '#4a4a4a', text: '#f0ece6', muted: '#a19a91' }
const KIND_TINT: Partial<Record<Legend['kind'], string>> = { mod: '#6ea8ff', media: '#c58bff', macro: '#3fbf7a', special: '#ff6b6b' }
const FONT = `Inter, ui-sans-serif, system-ui, -apple-system, 'Segoe UI', sans-serif`

const escape = (text: string) => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
const n = (value: number) => Math.round(value * 10) / 10

function mix(a: string, b: string, share: number): string {
  const parts = (hex: string) => [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16))
  const [x, y] = [parts(a), parts(b)]
  return '#' + x.map((part, i) => Math.round(part * share + y[i] * (1 - share)).toString(16).padStart(2, '0')).join('')
}

function text(x: number, y: number, size: number, fill: string, content: string, extra = ''): string {
  return `<text x="${n(x)}" y="${n(y)}" font-size="${n(size)}" fill="${fill}" ${extra}>${escape(content)}</text>`
}

const fit = (content: string, width: number, most: number, least = 8) => Math.max(least, Math.min(most, width / Math.max(content.length, 1) / 0.6))

/** What a layer does at a position, looking through see-through keys. */
function resolved(layers: Action[][], layer: number, pos: number): { action: Action; through: boolean } {
  for (let at = layer; at >= 0; at--) {
    const action = layers[at]?.[pos] ?? 'Transparent'
    if (action !== 'Transparent') return { action, through: at !== layer }
  }
  return { action: 'Transparent', through: true }
}

/** One keyboard, drawn at `scale` pixels per layout unit with its top left at (0, 0). */
function board(source: PosterSource, scale: number, legends: (pos: number) => string, cap: (pos: number) => { fill: string; edge: string; dashed: boolean }): { body: string; w: number; h: number } {
  const box = bounds(source.layout)
  const out: string[] = []
  source.layout.forEach((key, pos) => {
    if (!hasKey(key)) return
    const x = (key.x - box.x) * scale
    const y = (key.y - box.y) * scale
    const w = key.w * scale
    const h = key.h * scale
    const turn = key.r ? ` transform="rotate(${key.r / 10} ${n(x + w / 2)} ${n(y + h / 2)})"` : ''
    const look = cap(pos)
    const dash = look.dashed ? ' stroke-dasharray="4 4"' : ''
    if (source.knobs.includes(pos)) {
      const r = Math.min(w, h) / 2 - 3
      out.push(
        `<g${turn}><circle cx="${n(x + w / 2)}" cy="${n(y + h / 2)}" r="${n(r)}" fill="${look.fill}" stroke="${look.edge}" stroke-width="1.5"/>` +
          `<line x1="${n(x + w / 2)}" y1="${n(y + h / 2 - r)}" x2="${n(x + w / 2)}" y2="${n(y + h / 2 + r)}" stroke="${look.edge}" stroke-width="1"/>` +
          `<g transform="translate(${n(x)} ${n(y)})">${legends(pos)}</g></g>`,
      )
      return
    }
    out.push(
      `<g${turn}><rect x="${n(x + 2)}" y="${n(y + 2)}" width="${n(w - 4)}" height="${n(h - 4)}" rx="${n(scale * 11)}" fill="${look.fill}" stroke="${look.edge}" stroke-width="1.5"${dash}/>` +
        `<g transform="translate(${n(x)} ${n(y)})">${legends(pos)}</g></g>`,
    )
  })
  return { body: out.join(''), w: box.w * scale, h: box.h * scale }
}

function capLook(theme: Theme, colors: number[], shown: Legend | undefined, through: boolean) {
  if (!shown || shown.kind === 'empty' || through) return { fill: 'none', edge: theme.edge, dashed: true }
  const layerTint = shown.layer !== undefined ? layerHex(colors, shown.layer) : undefined
  const tint = layerTint ?? KIND_TINT[shown.kind]
  return { fill: tint ? mix(tint, theme.cap, layerTint ? 0.38 : 0.2) : theme.cap, edge: layerTint ? mix(layerTint, theme.edge, 0.7) : theme.edge, dashed: false }
}

/** The lines printed on one key of a layer's own keyboard. */
function plainLegend(theme: Theme, key: KeyGeometry, scale: number, shown: Legend | undefined, faint: boolean): string {
  if (!shown?.main && !shown?.sub) return ''
  const w = key.w * scale
  const h = key.h * scale
  const fill = faint ? theme.muted : theme.text
  const opacity = faint ? ' opacity="0.55"' : ''
  const main = text(w / 2, shown.sub ? h / 2 - 6 : h / 2, fit(shown.main, w - 12, 19), fill, shown.main, `font-weight="600"${opacity}`)
  const sub = shown.sub ? text(w / 2, h / 2 + 12, fit(shown.sub, w - 8, 9.5, 6.5), theme.muted, shown.sub, opacity) : ''
  return main + sub
}

function knobLegend(theme: Theme, key: KeyGeometry, scale: number, left: string, right: string): string {
  const w = key.w * scale
  const h = key.h * scale
  const size = (content: string) => fit(content, w / 2 - 8, 12, 6.5)
  return (
    text(w / 4 + 2, h / 2 - 10, 9, theme.muted, '⟲') + text((3 * w) / 4 - 2, h / 2 - 10, 9, theme.muted, '⟳') +
    text(w / 4 + 2, h / 2 + 4, size(left), theme.text, left, 'font-weight="600"') + text((3 * w) / 4 - 2, h / 2 + 4, size(right), theme.text, right, 'font-weight="600"')
  )
}

const layerTitle = (index: number) => (index === 0 ? 'Layer 0 · base' : `Layer ${index}`)

export function keymapPoster(source: PosterSource, options: PosterOptions): Poster {
  const theme = options.dark ? DARK : LIGHT
  const colors = source.layerColors
  const PAD = 28
  const parts: string[] = []
  let width = 0
  let y = PAD

  const title = source.name || 'My keyboard'
  parts.push(text(PAD, y + 12, 24, theme.text, title, 'font-weight="700" text-anchor="start"'))
  y += 44

  const dot = (x: number, cy: number, layer: number) => `<circle cx="${n(x)}" cy="${n(cy)}" r="6" fill="${layerHex(colors, layer) ?? theme.muted}"/>`

  if (options.mode === 'grid') {
    const scale = 0.56
    const box = bounds(source.layout)
    const panelW = box.w * scale + 32
    const panelH = box.h * scale + 62
    const count = source.layers.length
    // Wide keyboards go under each other, small ones side by side.
    const columns = Math.max(1, Math.min(count, panelW > 620 ? (count > 4 ? 2 : 1) : panelW > 380 ? 2 : 3))
    source.layers.forEach((_, layer) => {
      const px = PAD + (layer % columns) * (panelW + 18)
      const py = y + Math.floor(layer / columns) * (panelH + 18)
      const drawn = board(
        source,
        scale,
        (pos) => {
          const key = source.layout[pos]
          if (source.knobs.includes(pos)) {
            const side = (at: number) => legend(resolved(source.layers, layer, at).action).main
            return knobLegend(theme, key, scale, side(pos), side(pos + 1))
          }
          const { action, through } = resolved(source.layers, layer, pos)
          return plainLegend(theme, key, scale, legend(action), through)
        },
        (pos) => {
          const { action, through } = resolved(source.layers, layer, pos)
          return capLook(theme, colors, legend(action), through && !source.knobs.includes(pos))
        },
      )
      const tint = layerHex(colors, layer)
      parts.push(
        `<g transform="translate(${n(px)} ${n(py)})">` +
          `<rect width="${n(panelW)}" height="${n(panelH)}" rx="14" fill="${theme.panel}" stroke="${tint ? mix(tint, theme.panel, 0.6) : theme.edge}" stroke-width="1.5"/>` +
          dot(22, 24, layer) + text(36, 24, 14, theme.text, layerTitle(layer), 'font-weight="600" text-anchor="start"') +
          `<g transform="translate(16 46)">${drawn.body}</g></g>`,
      )
    })
    width = PAD * 2 + columns * panelW + (columns - 1) * 18
    y += Math.ceil(count / columns) * (panelH + 18)
  } else {
    // One keyboard: the base layer in the middle of each key, the others around it in their colours.
    const scale = 0.92
    const SPOTS: [number, number, string][] = [[0.2, 0.19, 'start'], [0.8, 0.19, 'end'], [0.2, 0.84, 'start'], [0.8, 0.84, 'end'], [0.5, 0.19, 'middle'], [0.5, 0.84, 'middle'], [0.8, 0.5, 'end']]
    let lx = PAD
    source.layers.forEach((_, layer) => {
      const label = layerTitle(layer)
      parts.push(dot(lx + 6, y + 6, layer) + text(lx + 18, y + 6, 13, theme.text, label, 'text-anchor="start"'))
      lx += 30 + label.length * 7.4
    })
    y += 30
    const drawn = board(
      source,
      scale,
      (pos) => {
        const key = source.layout[pos]
        const w = key.w * scale
        const h = key.h * scale
        if (source.knobs.includes(pos)) {
          const side = (at: number) => legend(resolved(source.layers, 0, at).action).main
          return knobLegend(theme, key, scale, side(pos), side(pos + 1))
        }
        const base = legend(source.layers[0]?.[pos] ?? 'Transparent')
        const out = [text(w / 2, h / 2 - (base.sub ? 3 : 0), fit(base.main, w - 16, 20), theme.text, base.kind === 'through' ? '' : base.main, 'font-weight="700"')]
        if (base.sub) out.push(text(w / 2, h / 2 + 12, fit(base.sub, w - 10, 9, 6.5), theme.muted, base.sub))
        source.layers.slice(1, SPOTS.length + 1).forEach((actions, index) => {
          const action = actions[pos] ?? 'Transparent'
          if (action === 'Transparent') return
          const shown = legend(action)
          const label = shown.main || (shown.kind === 'empty' ? '∅' : '')
          const [fx, fy, anchor] = SPOTS[index]
          const room = anchor === 'middle' ? w * 0.3 : w * 0.42
          out.push(text(fx * w + (anchor === 'start' ? -w * 0.11 : anchor === 'end' ? w * 0.11 : 0), fy * h, fit(label, room, 13, 7), layerHex(colors, index + 1) ?? theme.muted, label, `font-weight="700" text-anchor="${anchor}"`))
        })
        return out.join('')
      },
      (pos) => capLook(theme, colors, { ...legend(source.layers[0]?.[pos] ?? 'Transparent'), layer: undefined }, false),
    )
    parts.push(`<g transform="translate(${PAD} ${n(y)})">${drawn.body}</g>`)
    width = Math.max(PAD * 2 + drawn.w, lx + PAD)
    y += drawn.h + 18
  }

  const combos = source.combos.filter((combo) => combo.keys.length >= 2)
  if (combos.length) {
    parts.push(text(PAD, y + 8, 14, theme.text, 'Combos', 'font-weight="600" text-anchor="start"'))
    y += 28
    const name = (pos: number) => legend(resolved(source.layers, 0, pos).action).main || `key ${pos + 1}`
    const lines = combos.map((combo) => `${combo.keys.map(name).join(' + ')}  →  ${describeAction(combo.action)}`)
    const columnW = Math.max(...lines.map((line) => line.length)) * 7 + 30
    const columns = Math.max(1, Math.floor((width - PAD * 2) / columnW))
    lines.forEach((line, index) => {
      parts.push(text(PAD + (index % columns) * columnW, y + Math.floor(index / columns) * 20, 12.5, theme.muted, line, 'text-anchor="start"'))
    })
    y += Math.ceil(lines.length / columns) * 20
  }

  const height = y + PAD - 10
  width = Math.max(width, PAD * 2 + title.length * 14)
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${n(width)}" height="${n(height)}" viewBox="0 0 ${n(width)} ${n(height)}" font-family="${FONT}" text-anchor="middle" dominant-baseline="central">` +
    `<rect width="100%" height="100%" fill="${theme.bg}"/>${parts.join('')}</svg>`
  return { svg, width, height }
}

/** Offer a file to save. */
export function saveBlob(blob: Blob, name: string) {
  const link = document.createElement('a')
  link.href = URL.createObjectURL(blob)
  link.download = name
  link.click()
  setTimeout(() => URL.revokeObjectURL(link.href), 1000)
}

/** The poster as a PNG at twice its size, sharp enough to print. */
export async function posterPng(poster: Poster, zoom = 2): Promise<Blob> {
  const url = URL.createObjectURL(new Blob([poster.svg], { type: 'image/svg+xml' }))
  try {
    const image = new Image()
    await new Promise<void>((done, failed) => {
      image.onload = () => done()
      image.onerror = () => failed(new Error('The picture could not be drawn.'))
      image.src = url
    })
    const canvas = document.createElement('canvas')
    canvas.width = Math.round(poster.width * zoom)
    canvas.height = Math.round(poster.height * zoom)
    canvas.getContext('2d')!.drawImage(image, 0, 0, canvas.width, canvas.height)
    return await new Promise<Blob>((done, failed) => canvas.toBlob((blob) => (blob ? done(blob) : failed(new Error('The picture could not be saved.'))), 'image/png'))
  } finally {
    URL.revokeObjectURL(url)
  }
}
