<script setup lang="ts">
// Arrange the keys on screen the way they sit on the desk. Every change goes
// to the keyboard when the gesture ends; the editor's undo takes it back.
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { attempt, toast } from '../lib/app'
import { legend } from '../lib/keycodes'
import { bounds, corners, gridLayout, hasKey, normalised, parseKle, UNIT, type Bounds } from '../lib/layout'
import type { KeyGeometry } from '../lib/protocol'
import { knobPositions, knobSpot, setLayout, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

const SNAP = UNIT / 4
const draft = ref<KeyGeometry[]>(copy(props.session.layout))
const selection = reactive(new Set<number>())
const followPresses = ref(true)
const importing = ref(false)
const kleText = ref('')

function copy(layout: KeyGeometry[]): KeyGeometry[] {
  return layout.map((key) => ({ ...key }))
}

// The layout on the keyboard can change underneath (an undo, the other half added).
watch(
  () => props.session.layout,
  (layout) => {
    if (dragging) return
    draft.value = copy(layout)
    for (const pos of [...selection]) if (!hasKey(draft.value[pos])) selection.delete(pos)
    view.value = fit()
  },
)

/** Give the keyboard the picture as it is drawn now. */
function commit() {
  const tidy = normalised(draft.value)
  draft.value = copy(tidy)
  view.value = fit()
  if (JSON.stringify(tidy) !== JSON.stringify(props.session.layout)) attempt(() => setLayout(props.session, tidy))
}

const keys = computed(() => draft.value.map((key, pos) => ({ key, pos })).filter(({ key }) => hasKey(key)))
/** Knobs have a place in the picture too; it lives at the first of their two positions. */
const knobs = computed(() => knobPositions(props.session))
const isKnob = (pos: number) => knobs.value.includes(pos)
const missingKnobs = computed(() => knobs.value.filter((pos) => !hasKey(draft.value[pos])))
const switches = computed(() => keys.value.filter(({ pos }) => !isKnob(pos)))

function placeKnob(pos: number) {
  while (draft.value.length <= pos + 1) draft.value.push({ x: 0, y: 0, w: 0, h: 0, r: 0 })
  draft.value[pos] = knobSpot(draft.value)
  selection.clear()
  selection.add(pos)
  commit()
}

const chosen = computed(() => [...selection].map((pos) => draft.value[pos]).filter(hasKey))

// The view only changes when asked, so keys do not slide away mid-drag.
const view = ref<Bounds>(fit())
function fit(): Bounds {
  const box = bounds(draft.value)
  const pad = UNIT * 1.5
  return { x: box.x - pad, y: box.y - pad, w: box.w + 2 * pad, h: box.h + 2 * pad }
}
const gridStart = computed(() => ({
  x: Math.floor(view.value.x / UNIT) * UNIT,
  y: Math.floor(view.value.y / UNIT) * UNIT,
}))

// --- Dragging ----------------------------------------------------------------
const svg = ref<SVGSVGElement>()
let dragging: { startX: number; startY: number; origin: Map<number, { x: number; y: number }> } | undefined

function toWorld(event: PointerEvent): { x: number; y: number } {
  const matrix = svg.value!.getScreenCTM()!.inverse()
  const point = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix)
  return { x: point.x, y: point.y }
}

function down(pos: number, event: PointerEvent) {
  event.stopPropagation()
  if (event.shiftKey) {
    if (selection.has(pos)) selection.delete(pos)
    else selection.add(pos)
  } else if (!selection.has(pos)) {
    selection.clear()
    selection.add(pos)
  }
  const start = toWorld(event)
  dragging = {
    startX: start.x,
    startY: start.y,
    origin: new Map([...selection].map((p) => [p, { x: draft.value[p].x, y: draft.value[p].y }])),
  }
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up, { once: true })
}

function move(event: PointerEvent) {
  if (!dragging) return
  const now = toWorld(event)
  const snap = (value: number) => Math.round(value / SNAP) * SNAP
  const dx = snap(now.x - dragging.startX)
  const dy = snap(now.y - dragging.startY)
  for (const [pos, origin] of dragging.origin) {
    draft.value[pos].x = origin.x + dx
    draft.value[pos].y = origin.y + dy
  }
}

function up() {
  window.removeEventListener('pointermove', move)
  dragging = undefined
  commit()
}

// --- Selecting with a rectangle, as in a file manager ---------------------------
const marquee = ref<Bounds>()
let band: { x: number; y: number; kept: number[] } | undefined

function bandDown(event: PointerEvent) {
  const start = toWorld(event)
  // With Shift the rectangle adds to what is selected already.
  band = { ...start, kept: event.shiftKey ? [...selection] : [] }
  if (!event.shiftKey) selection.clear()
  window.addEventListener('pointermove', bandMove)
  window.addEventListener('pointerup', bandUp, { once: true })
}

function bandMove(event: PointerEvent) {
  if (!band) return
  const now = toWorld(event)
  const box = { x: Math.min(band.x, now.x), y: Math.min(band.y, now.y), w: Math.abs(now.x - band.x), h: Math.abs(now.y - band.y) }
  marquee.value = box
  selection.clear()
  band.kept.forEach((pos) => selection.add(pos))
  // Touching a key is enough; it need not be inside completely.
  for (const { key, pos } of keys.value) {
    const points = corners(key)
    const xs = points.map((p) => p[0])
    const ys = points.map((p) => p[1])
    const apart = Math.max(...xs) < box.x || Math.min(...xs) > box.x + box.w || Math.max(...ys) < box.y || Math.min(...ys) > box.y + box.h
    if (!apart) selection.add(pos)
  }
}

function bandUp() {
  window.removeEventListener('pointermove', bandMove)
  band = undefined
  marquee.value = undefined
}

// --- Keyboard shortcuts ------------------------------------------------------
function onKeydown(event: KeyboardEvent) {
  if ((event.target as HTMLElement).closest('input, textarea, select')) return
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a') {
    keys.value.forEach(({ pos }) => selection.add(pos))
    event.preventDefault()
    return
  }
  if (!selection.size) return
  if (event.key === 'Escape') {
    selection.clear()
    return
  }
  const step = event.shiftKey ? 5 : SNAP
  const moves: Record<string, [number, number]> = {
    ArrowLeft: [-step, 0],
    ArrowRight: [step, 0],
    ArrowUp: [0, -step],
    ArrowDown: [0, step],
  }
  const delta = moves[event.key]
  if (delta) {
    chosen.value.forEach((key) => ((key.x += delta[0]), (key.y += delta[1])))
    commit()
    event.preventDefault()
  } else if (event.key === 'r' || event.key === 'R') {
    rotate(event.shiftKey ? -150 : 150)
  }
}
onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('pointermove', move)
  window.removeEventListener('pointermove', bandMove)
})

// Pressing a key on the real keyboard selects it here.
watch(
  () => [...props.session.pressed],
  (now, before) => {
    if (!followPresses.value) return
    const fresh = now.find((pos) => !before.includes(pos) && hasKey(draft.value[pos]))
    if (fresh !== undefined) {
      selection.clear()
      selection.add(fresh)
    }
  },
)

// --- Tools -------------------------------------------------------------------
function rotate(tenths: number) {
  chosen.value.forEach((key) => (key.r = ((key.r + tenths + 1800) % 3600) - 1800))
  commit()
}

function setField(field: 'x' | 'y' | 'w' | 'h' | 'r', event: Event) {
  const value = Number((event.target as HTMLInputElement).value)
  if (Number.isNaN(value)) return
  const scaled = field === 'r' ? Math.round(value * 10) : Math.round(value * UNIT)
  if ((field === 'x' || field === 'y') && chosen.value.length > 1) {
    // Several keys: move them together so the group's edge lands there.
    const delta = scaled - Math.min(...chosen.value.map((key) => key[field]))
    chosen.value.forEach((key) => (key[field] += delta))
  } else if (field === 'w') {
    reflowWidths(Math.max(SNAP, scaled))
  } else {
    chosen.value.forEach((key) => (key[field] = field === 'h' ? Math.max(SNAP, scaled) : scaled))
  }
  commit()
}

/**
 * Give every selected key this width and let its row follow:
 * - a wider key first grows into the free space on its right; a key it
 *   would run into is pushed, just far enough;
 * - a key that sat directly against its left neighbour stays against it,
 *   so it is pulled along when that neighbour gets narrower.
 * Keys beyond a gap that is not used up stay where they are.
 */
function reflowWidths(width: number) {
  const all = draft.value.filter(hasKey)
  const before = new Map(all.map((key) => [key, { x: key.x, right: key.x + key.w }]))
  const middle = (key: KeyGeometry) => key.y + key.h / 2
  const sameRow = (a: KeyGeometry, b: KeyGeometry) => Math.abs(middle(a) - middle(b)) < Math.min(a.h, b.h) / 2
  chosen.value.forEach((key) => (key.w = width))
  // Left to right, so every key to the left already has its final place.
  const ordered = [...all].sort((a, b) => before.get(a)!.x - before.get(b)!.x)
  for (const [index, key] of ordered.entries()) {
    const was = before.get(key)!
    const lefts = ordered.slice(0, index).filter((left) => sameRow(left, key))
    const attached = lefts.filter((left) => Math.abs(before.get(left)!.right - was.x) <= 1)
    if (attached.length) key.x = Math.max(...attached.map((left) => left.x + left.w))
    for (const left of lefts) {
      // Keys that overlapped already may keep overlapping by as much.
      const overlapBefore = Math.max(0, before.get(left)!.right - was.x)
      key.x = Math.max(key.x, left.x + left.w - overlapBefore)
    }
  }
}

const shown = (field: 'x' | 'y' | 'w' | 'h' | 'r') => {
  if ((field === 'x' || field === 'y') && chosen.value.length > 1) return Math.min(...chosen.value.map((key) => key[field])) / UNIT
  const values = new Set(chosen.value.map((key) => key[field]))
  if (values.size !== 1) return ''
  const [value] = values
  return field === 'r' ? value / 10 : value / UNIT
}

function mirror() {
  const targets = chosen.value.length ? chosen.value : draft.value.filter(hasKey)
  const box = bounds(targets)
  targets.forEach((key) => {
    key.x = Math.round(box.x + box.w - (key.x - box.x) - key.w)
    key.r = -key.r
  })
  commit()
}

function resetGrid() {
  const populated = switches.value.map(({ pos }) => pos)
  const grid = gridLayout({ Direct: { pins: Array(populated.length).fill(0), active_high: false } })
  populated.forEach((pos, index) => (draft.value[pos] = grid[index]))
  knobs.value.filter((pos) => hasKey(draft.value[pos])).forEach((pos) => {
    draft.value[pos] = { x: 0, y: 0, w: 0, h: 0, r: 0 }
    draft.value[pos] = knobSpot(draft.value)
  })
  commit()
}

function importKle() {
  try {
    const shapes = parseKle(kleText.value)
    const targets = switches.value
    if (shapes.length !== targets.length) {
      throw new Error(`That layout has ${shapes.length} keys, but this keyboard has ${targets.length}.`)
    }
    // Keys are matched in order: first key of the layout to the first position.
    targets.forEach(({ pos }, index) => (draft.value[pos] = shapes[index]))
    importing.value = false
    kleText.value = ''
    commit()
  } catch (error) {
    toast(error instanceof Error ? error.message : String(error), 'error')
  }
}

const label = (pos: number) => (props.session.layers[0]?.[pos] ? legend(props.session.layers[0][pos]).main : '')
</script>

<template>
  <div class="layout">
    <div class="canvas-wrap">
      <svg
        ref="svg"
        class="canvas"
        :viewBox="`${view.x} ${view.y} ${view.w} ${view.h}`"
        data-test="layout-canvas"
        @pointerdown="bandDown"
      >
        <defs>
          <pattern id="grid" :x="gridStart.x" :y="gridStart.y" :width="UNIT" :height="UNIT" patternUnits="userSpaceOnUse">
            <path :d="`M ${UNIT} 0 H 0 V ${UNIT}`" class="grid-line" />
          </pattern>
        </defs>
        <rect :x="view.x" :y="view.y" :width="view.w" :height="view.h" fill="url(#grid)" />
        <g
          v-for="{ key, pos } in keys"
          :key="pos"
          class="key"
          :class="{ selected: selection.has(pos), pressed: session.pressed.has(pos), knob: isKnob(pos) }"
          :transform="`rotate(${key.r / 10} ${key.x + key.w / 2} ${key.y + key.h / 2})`"
          :data-pos="pos"
          @pointerdown="down(pos, $event)"
        >
          <rect :x="key.x + 3" :y="key.y + 3" :width="key.w - 6" :height="key.h - 6" :rx="isKnob(pos) ? Math.min(key.w, key.h) / 2 : 12" />
          <text class="main" :x="key.x + key.w / 2" :y="key.y + key.h / 2 - 4">{{ isKnob(pos) ? '⟲ ⟳' : label(pos) }}</text>
          <text class="index" :x="key.x + key.w / 2" :y="key.y + key.h - 20">{{ isKnob(pos) ? 'knob' : pos }}</text>
        </g>
        <rect v-if="marquee" class="marquee" :x="marquee.x" :y="marquee.y" :width="marquee.w" :height="marquee.h" rx="4" data-test="marquee" />
      </svg>
    </div>

    <aside class="side card col scroll">
      <div>
        <h2>Layout</h2>
        <p class="muted small">
          Drag keys to where they sit on your keyboard. Drag across the background to select several; arrow keys
          nudge, R rotates. Changes are kept as you make them.
        </p>
      </div>

      <label class="row check">
        <input v-model="followPresses" type="checkbox" />
        <span>Pressing a key on the keyboard selects it here</span>
      </label>

      <div v-if="selection.size" class="col">
        <h3>{{ selection.size === 1 ? (isKnob([...selection][0]) ? 'Knob' : `Key ${[...selection][0]}`) : `${selection.size} keys` }}</h3>
        <div class="fields">
          <label v-for="field in ['x', 'y', 'w', 'h', 'r'] as const" :key="field" class="field">
            {{ { x: 'Left', y: 'Top', w: 'Width', h: 'Height', r: 'Angle °' }[field] }}
            <input class="input" type="number" :step="field === 'r' ? 5 : 0.25" :value="shown(field)" :placeholder="selection.size > 1 ? 'mixed' : ''" :data-test="'field-' + field" @change="setField(field, $event)" />
          </label>
        </div>
        <div class="row">
          <button class="btn small" @click="rotate(-150)">⟲ 15°</button>
          <button class="btn small" @click="rotate(150)">⟳ 15°</button>
        </div>
        <p class="muted small">
          <template v-if="selection.size > 1">Left and Top move the group as one; Width, Height and Angle are set on every selected key. </template>
          A wider key grows into the gap on its right and pushes its neighbours only when it would touch them. Keys
          sitting directly against it follow when it gets narrower.
        </p>
      </div>
      <p v-else class="muted small">Select a key to change its size or angle.</p>

      <div class="col tools">
        <h3>Tools</h3>
        <button v-for="(pos, index) in missingKnobs" :key="pos" class="btn small" :data-test="'place-knob-' + index" @click="placeKnob(pos)">
          Put {{ knobs.length > 1 ? `knob ${knobs.indexOf(pos) + 1}` : 'the knob' }} in the picture
        </button>
        <button class="btn small" data-test="mirror" @click="mirror">Mirror {{ selection.size ? 'selection' : 'everything' }}</button>
        <button class="btn small" data-test="reset-grid" @click="resetGrid">Reset to a plain grid</button>
        <button class="btn small" data-test="import-kle" @click="importing = !importing">Import from keyboard-layout-editor</button>
        <Transition name="rise">
          <div v-if="importing" class="col">
            <textarea v-model="kleText" class="input" rows="5" placeholder='Paste the "Raw data" here' data-test="kle-text" />
            <button class="btn small primary" :disabled="!kleText.trim()" data-test="kle-apply" @click="importKle">Use this layout</button>
          </div>
        </Transition>
      </div>
    </aside>
  </div>
</template>

<style scoped>
.layout {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 320px;
  gap: 18px;
  height: 100%;
  min-height: 0;
  padding: 18px 24px;
}

.canvas-wrap {
  min-height: 0;
  border: 1px solid var(--line);
  border-radius: var(--radius);
  background: var(--panel);
  overflow: hidden;
}

.canvas {
  width: 100%;
  height: 100%;
  touch-action: none;
  user-select: none;
}

.grid-line {
  fill: none;
  stroke: var(--line);
  stroke-width: 1.5;
  opacity: 0.6;
}

.key {
  cursor: grab;
}

.marquee {
  fill: var(--accent-soft);
  stroke: var(--accent);
  stroke-width: 2;
  stroke-dasharray: 8 6;
  pointer-events: none;
}

.key rect {
  fill: var(--cap-top);
  stroke: var(--cap);
  stroke-width: 4;
  transition:
    stroke 0.15s,
    fill 0.15s;
}

.key:hover rect {
  stroke: var(--muted);
}

.key.selected rect {
  stroke: var(--accent);
}

.key.pressed rect {
  fill: var(--accent);
}

text {
  text-anchor: middle;
  dominant-baseline: central;
  pointer-events: none;
  fill: var(--cap-text);
}

.main {
  font-size: 24px;
  font-weight: 600;
}

.index {
  font-size: 12px;
  opacity: 0.45;
}

.side {
  min-height: 0;
  gap: 16px;
}

.small {
  font-size: 0.85rem;
}

.check {
  font-size: 0.9rem;
  cursor: pointer;
}

.check input {
  accent-color: var(--accent);
}

.fields {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.tools {
  align-items: stretch;
}

textarea {
  resize: vertical;
  font-family: ui-monospace, Menlo, Consolas, monospace;
  font-size: 0.8rem;
}
</style>
