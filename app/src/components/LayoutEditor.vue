<script setup lang="ts">
// Arranging the keys the way they sit on the desk, in the sheet under the
// keymap's keyboard. The keymap draws the keyboard from `draft` while this is
// open and hands over what is pressed on it. Every change goes to the keyboard
// when the gesture ends; the editor's undo takes it back.
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { attempt, toast } from '../lib/app'
import { bounds, gridLayout, hasKey, knobKey, NO_KEY, normalised, parseKle, UNIT } from '../lib/layout'
import type { KeyGeometry } from '../lib/protocol'
import { knobPositions, knobSpot, setLayout, type Session } from '../lib/session'
import BottomSheet from './BottomSheet.vue'

const props = defineProps<{ session: Session }>()
/** The keymap's selection: what is selected there is selected here. */
const selection = defineModel<number[]>('selection', { required: true })
const minimized = defineModel<boolean>('minimized', { required: true })

const SNAP = UNIT / 4
const FIELDS = { x: 'Left', y: 'Top', w: 'Width', h: 'Height', r: 'Angle °' } as const
type Field = keyof typeof FIELDS
const copy = (layout: KeyGeometry[]): KeyGeometry[] => layout.map((key) => ({ ...key }))

const draft = ref(copy(props.session.layout))
/** The part of the picture shown. It only changes between gestures, so keys do not slide away mid-drag. */
const frame = ref(bounds(draft.value))
const followPresses = ref(true)
const kleText = ref('')

/** Knobs have a place in the picture too; it lives at the first of their two positions. */
const knobs = computed(() => knobPositions(props.session))
const isKnob = (pos: number) => knobs.value.includes(pos)
/** Knobs that sit on a switch, which is then their press: knob position to key position. */
const carried = reactive(new Map<number, number>())
function findCarried() {
  carried.clear()
  knobs.value.forEach((knob) => {
    const key = knobKey(draft.value, knobs.value, knob)
    if (key >= 0) carried.set(knob, key)
  })
}
findCarried()
const knobOn = (pos: number) => [...carried].find(([, key]) => key === pos)?.[0]
/** A knob on a key goes where its key goes. */
const carry = () => carried.forEach((key, knob) => (draft.value[knob] = { ...draft.value[key] }))
const missingKnobs = computed(() => knobs.value.filter((pos) => !hasKey(draft.value[pos])))
const switches = computed(() => draft.value.flatMap((key, pos) => (hasKey(key) && !isKnob(pos) ? [pos] : [])))

/** What a position is in the picture: either turn of a knob is the knob, and a knob on a key is that key. */
function place(pos: number): number {
  const knob = knobs.value.find((at) => at === pos || at + 1 === pos)
  return knob === undefined ? pos : (carried.get(knob) ?? knob)
}
/** The selected places, each once. */
const picked = computed(() => [...new Set(selection.value.map(place))].filter((pos) => hasKey(draft.value[pos])))
const chosen = computed(() => picked.value.map((pos) => draft.value[pos]))
const only = computed(() => (picked.value.length === 1 ? picked.value[0] : undefined))
const heading = computed(() => (only.value === undefined ? (picked.value.length ? `${picked.value.length} keys` : 'No key selected') : isKnob(only.value) ? 'Knob' : `Key ${only.value}`))

// The layout on the keyboard can change underneath (an undo, the other half added).
watch(
  () => props.session.layout,
  (layout) => {
    if (dragging) return
    draft.value = copy(layout)
    findCarried()
    frame.value = bounds(draft.value)
  },
)

/** Give the keyboard the picture as it is drawn now. */
function commit() {
  carry()
  const tidy = normalised(draft.value)
  draft.value = copy(tidy)
  frame.value = bounds(tidy)
  if (JSON.stringify(tidy) !== JSON.stringify(props.session.layout)) attempt(() => setLayout(props.session, tidy))
}

/** Put a knob on the selected key, or take the one there off again. */
function setKnob(pos: number, knob: number | undefined) {
  for (const [other, key] of [...carried]) {
    if (key !== pos && other !== knob) continue
    carried.delete(other)
    draft.value[other] = { ...NO_KEY }
    if (other !== knob) draft.value[other] = knobSpot(draft.value)
  }
  if (knob !== undefined) {
    while (draft.value.length <= knob + 1) draft.value.push({ ...NO_KEY })
    carried.set(knob, pos)
  }
  commit()
}

function placeKnob(pos: number) {
  while (draft.value.length <= pos + 1) draft.value.push({ ...NO_KEY })
  draft.value[pos] = knobSpot(draft.value)
  selection.value = [pos]
  commit()
}

// --- Dragging ----------------------------------------------------------------
let dragging: { move: (event: PointerEvent) => void } | undefined

/** A key of the keyboard picture was pressed: select it, and move what is selected along with the pointer. */
function down(pos: number, event: PointerEvent) {
  const at = place(pos)
  if (event.shiftKey || event.metaKey || event.ctrlKey) selection.value = picked.value.includes(at) ? picked.value.filter((each) => each !== at) : [...picked.value, at]
  else if (!picked.value.includes(at)) selection.value = [at]
  // Where the pointer is, in the picture's own units.
  const svg = (event.target as SVGElement).ownerSVGElement!
  const world = (now: PointerEvent) => new DOMPoint(now.clientX, now.clientY).matrixTransform(svg.getScreenCTM()!.inverse())
  const start = world(event)
  const origin = picked.value.map((each) => ({ key: draft.value[each], x: draft.value[each].x, y: draft.value[each].y }))
  const snap = (value: number) => Math.round(value / SNAP) * SNAP
  const move = (now: PointerEvent) => {
    const to = world(now)
    for (const { key, x, y } of origin) {
      key.x = x + snap(to.x - start.x)
      key.y = y + snap(to.y - start.y)
    }
    carry()
  }
  dragging = { move }
  window.addEventListener('pointermove', move)
  window.addEventListener(
    'pointerup',
    () => {
      window.removeEventListener('pointermove', move)
      dragging = undefined
      commit()
    },
    { once: true },
  )
}

// --- Keyboard shortcuts ------------------------------------------------------
function onKeydown(event: KeyboardEvent) {
  if ((event.target as HTMLElement).closest('input, textarea, select')) return
  if (event.metaKey || event.ctrlKey) {
    if (event.key.toLowerCase() !== 'a') return
    selection.value = draft.value.flatMap((key, pos) => (hasKey(key) && !carried.has(pos) ? [pos] : []))
    event.preventDefault()
    return
  }
  if (!picked.value.length) return
  if (event.key === 'Escape') {
    selection.value = []
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
  if (dragging) window.removeEventListener('pointermove', dragging.move)
})

// Pressing a key on the real keyboard selects it here.
watch(
  () => [...props.session.pressed],
  (now, before) => {
    if (!followPresses.value) return
    const fresh = now.find((pos) => !before.includes(pos) && hasKey(draft.value[pos]))
    if (fresh !== undefined) selection.value = [fresh]
  },
)

// --- Tools -------------------------------------------------------------------
function rotate(tenths: number) {
  chosen.value.forEach((key) => (key.r = ((key.r + tenths + 1800) % 3600) - 1800))
  commit()
}

function setField(field: Field, event: Event) {
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

const shown = (field: Field) => {
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
  const populated = switches.value
  const grid = gridLayout({ Direct: { pins: Array(populated.length).fill(0), active_high: false } })
  populated.forEach((pos, index) => (draft.value[pos] = grid[index]))
  knobs.value.filter((pos) => hasKey(draft.value[pos])).forEach((pos) => {
    draft.value[pos] = { ...NO_KEY }
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
    targets.forEach((pos, index) => (draft.value[pos] = shapes[index]))
    kleText.value = ''
    commit()
  } catch (error) {
    toast(error instanceof Error ? error.message : String(error), 'error')
  }
}

defineExpose({
  draft,
  frame,
  down,
  /** What to mark as selected in the picture: a knob with both its turns. */
  selected: computed(() => picked.value.flatMap((pos) => (isKnob(pos) ? [pos, pos + 1] : [pos]))),
})
</script>

<template>
  <BottomSheet v-model:minimized="minimized" data-test="layout-tools">
    <div class="arrange" :class="{ minimized }">
      <div class="bar row">
        <div class="now">
          <span class="muted small">Layout</span>
          <strong data-test="layout-target">{{ heading }}</strong>
        </div>
        <label v-if="!minimized" class="row check">
          <input v-model="followPresses" type="checkbox" />
          <span>Pressing a key on the keyboard selects it here</span>
        </label>
      </div>

      <div v-show="!minimized" class="tools scroll">
        <section class="col">
          <h3>Size and place</h3>
          <div class="fields">
            <label v-for="(name, field) in FIELDS" :key="field" class="field">
              {{ name }}
              <input class="input" type="number" :step="field === 'r' ? 5 : 0.25" :value="shown(field)" :disabled="!chosen.length" :placeholder="chosen.length > 1 ? 'mixed' : ''" :data-test="'field-' + field" @change="setField(field, $event)" />
            </label>
            <div class="row turns">
              <button class="btn small" :disabled="!chosen.length" title="Turn left by 15° (Shift R)" @click="rotate(-150)">⟲ 15°</button>
              <button class="btn small" :disabled="!chosen.length" title="Turn right by 15° (R)" @click="rotate(150)">⟳ 15°</button>
            </div>
          </div>
          <label v-if="knobs.length" class="field">
            Knob on this key (pressing the knob is this key)
            <select class="input" data-test="knob-on-key" :disabled="only === undefined || isKnob(only)" :value="only === undefined ? '' : (knobOn(only) ?? '')" @change="setKnob(only!, ($event.target as HTMLSelectElement).value === '' ? undefined : Number(($event.target as HTMLSelectElement).value))">
              <option value="">none</option>
              <option v-for="(knob, index) in knobs" :key="knob" :value="knob">knob {{ index + 1 }}</option>
            </select>
          </label>
          <p class="muted small">
            <template v-if="chosen.length > 1">Left and Top move the group as one; the rest is set on every selected key. </template>
            A wider key grows into the gap on its right and pushes its neighbours only when it would touch them.
          </p>
        </section>

        <section class="col">
          <h3>Whole keyboard</h3>
          <button v-for="(pos, index) in missingKnobs" :key="pos" class="btn small" :data-test="'place-knob-' + index" @click="placeKnob(pos)">
            Put {{ knobs.length > 1 ? `knob ${knobs.indexOf(pos) + 1}` : 'the knob' }} in the picture
          </button>
          <button class="btn small" data-test="mirror" @click="mirror">Mirror {{ chosen.length ? 'selection' : 'everything' }}</button>
          <button class="btn small" data-test="reset-grid" @click="resetGrid">Reset to a plain grid</button>
        </section>

        <section class="col">
          <h3>From keyboard-layout-editor</h3>
          <textarea v-model="kleText" class="input" placeholder='Paste the "Raw data" here' data-test="kle-text" />
          <button class="btn small primary" :disabled="!kleText.trim()" data-test="kle-apply" @click="importKle">Use this layout</button>
        </section>
      </div>
    </div>
  </BottomSheet>
</template>

<style scoped>
.arrange {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
  min-height: 0;
}

.small {
  font-size: 0.82rem;
}

.bar {
  gap: 16px;
  min-height: 40px;
  padding-right: var(--fold-room);
}

.now {
  display: flex;
  flex-direction: column;
  min-width: 110px;
  line-height: 1.25;
}

.minimized .bar {
  min-height: 0;
}

.minimized .now {
  flex-direction: row;
  align-items: baseline;
  gap: 8px;
}

.check {
  gap: 7px;
  font-size: 0.9rem;
  cursor: pointer;
}

.check input {
  accent-color: var(--accent);
}

.tools {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(300px, 1.3fr) minmax(190px, 0.8fr) minmax(220px, 1.2fr);
  gap: 24px;
  padding-top: 10px;
  border-top: 1px solid var(--line);
}

section {
  gap: 10px;
  align-items: stretch;
  min-height: 0;
}

.fields {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
  align-items: end;
}

/* A number box is as wide as its column, not as wide as it likes. */
.fields .input {
  width: 100%;
}

.turns {
  gap: 6px;
}

.turns .btn {
  flex: 1;
  padding: 8px 4px;
}

p {
  margin: 0;
}

textarea {
  flex: 1;
  min-height: 70px;
  resize: none;
  font-family: ui-monospace, Menlo, Consolas, monospace;
  font-size: 0.8rem;
}
</style>
