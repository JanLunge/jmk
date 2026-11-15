<script setup lang="ts">
// Arrange the keys on screen the way they sit on the desk.
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { attempt, toast } from '../lib/app'
import { legend } from '../lib/keycodes'
import { bounds, gridLayout, hasKey, normalised, parseKle, UNIT, type Bounds } from '../lib/layout'
import type { KeyGeometry } from '../lib/protocol'
import { setLayout, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

const SNAP = UNIT / 4
const draft = ref<KeyGeometry[]>(copy(props.session.layout))
const selection = reactive(new Set<number>())
const dirty = computed(() => JSON.stringify(draft.value) !== JSON.stringify(props.session.layout))
const followPresses = ref(true)
const importing = ref(false)
const kleText = ref('')

function copy(layout: KeyGeometry[]): KeyGeometry[] {
  return layout.map((key) => ({ ...key }))
}

// The layout on the keyboard can change underneath (other half added).
watch(() => props.session.layout, (layout) => !dragging && (draft.value = copy(layout)))

const keys = computed(() => draft.value.map((key, pos) => ({ key, pos })).filter(({ key }) => hasKey(key)))
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
  view.value = fit()
}

// --- Keyboard shortcuts ------------------------------------------------------
function onKeydown(event: KeyboardEvent) {
  if ((event.target as HTMLElement).closest('input, textarea, select') || !selection.size) return
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
    view.value = fit()
    event.preventDefault()
  } else if (event.key === 'r' || event.key === 'R') {
    rotate(event.shiftKey ? -150 : 150)
  }
}
onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('pointermove', move)
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
}

function setField(field: 'x' | 'y' | 'w' | 'h' | 'r', event: Event) {
  const value = Number((event.target as HTMLInputElement).value)
  if (Number.isNaN(value)) return
  const scaled = field === 'r' ? Math.round(value * 10) : Math.round(value * UNIT)
  chosen.value.forEach((key) => (key[field] = field === 'w' || field === 'h' ? Math.max(SNAP, scaled) : scaled))
  view.value = fit()
}

const shown = (field: 'x' | 'y' | 'w' | 'h' | 'r') => {
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
}

function resetGrid() {
  const populated = draft.value.map((key, pos) => (hasKey(key) ? pos : -1)).filter((pos) => pos >= 0)
  const grid = gridLayout({ Direct: { pins: Array(populated.length).fill(0), active_high: false } })
  populated.forEach((pos, index) => (draft.value[pos] = grid[index]))
  view.value = fit()
}

function importKle() {
  try {
    const shapes = parseKle(kleText.value)
    const targets = keys.value
    if (shapes.length !== targets.length) {
      throw new Error(`That layout has ${shapes.length} keys, but this keyboard has ${targets.length}.`)
    }
    // Keys are matched in order: first key of the layout to the first position.
    targets.forEach(({ pos }, index) => (draft.value[pos] = shapes[index]))
    importing.value = false
    kleText.value = ''
    view.value = fit()
  } catch (error) {
    toast(error instanceof Error ? error.message : String(error), 'error')
  }
}

async function apply() {
  const tidy = normalised(draft.value)
  if (await attempt(() => setLayout(props.session, tidy).then(() => true))) {
    draft.value = copy(tidy)
    view.value = fit()
  }
}

function revert() {
  draft.value = copy(props.session.layout)
  view.value = fit()
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
        @pointerdown="selection.clear()"
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
          :class="{ selected: selection.has(pos), pressed: session.pressed.has(pos) }"
          :transform="`rotate(${key.r / 10} ${key.x + key.w / 2} ${key.y + key.h / 2})`"
          :data-pos="pos"
          @pointerdown="down(pos, $event)"
        >
          <rect :x="key.x + 3" :y="key.y + 3" :width="key.w - 6" :height="key.h - 6" rx="12" />
          <text class="main" :x="key.x + key.w / 2" :y="key.y + key.h / 2 - 4">{{ label(pos) }}</text>
          <text class="index" :x="key.x + key.w / 2" :y="key.y + key.h - 20">{{ pos }}</text>
        </g>
      </svg>
    </div>

    <aside class="side card col scroll">
      <div>
        <h2>Layout</h2>
        <p class="muted small">
          Drag keys to where they sit on your keyboard. Shift-click selects several. Arrow keys nudge, R rotates.
        </p>
      </div>

      <label class="row check">
        <input v-model="followPresses" type="checkbox" />
        <span>Pressing a key on the keyboard selects it here</span>
      </label>

      <div v-if="selection.size" class="col">
        <h3>{{ selection.size === 1 ? `Key ${[...selection][0]}` : `${selection.size} keys` }}</h3>
        <div class="fields">
          <label v-for="field in ['x', 'y', 'w', 'h', 'r'] as const" :key="field" class="field">
            {{ { x: 'Left', y: 'Top', w: 'Width', h: 'Height', r: 'Angle °' }[field] }}
            <input class="input" type="number" :step="field === 'r' ? 5 : 0.25" :value="shown(field)" :data-test="'field-' + field" @change="setField(field, $event)" />
          </label>
        </div>
        <div class="row">
          <button class="btn small" @click="rotate(-150)">⟲ 15°</button>
          <button class="btn small" @click="rotate(150)">⟳ 15°</button>
        </div>
      </div>
      <p v-else class="muted small">Select a key to change its size or angle.</p>

      <div class="col tools">
        <h3>Tools</h3>
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

      <span class="grow" />
      <div class="row">
        <button class="btn primary grow" :disabled="!dirty" data-test="layout-apply" @click="apply">Apply layout</button>
        <button class="btn ghost" :disabled="!dirty" @click="revert">Revert</button>
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
  fill: var(--signal);
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
