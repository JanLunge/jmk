<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import ActionPicker from '../components/ActionPicker.vue'
import KeyboardView from '../components/KeyboardView.vue'
import KeymapOverview from '../components/KeymapOverview.vue'
import { app, attempt, toast } from '../lib/app'
import { localPositions } from '../lib/definition'
import { dropped, landOn } from '../lib/drag'
import { isModifierUsage, legend, usageForCode } from '../lib/keycodes'
import { LAYER_COLORS, layerColor, layerColorNumber } from '../lib/layers'
import { hasKey } from '../lib/layout'
import type { Action, Basic } from '../lib/protocol'
import { canColorLayers, knobPositions, positionCount, setAction, setLayerColor, setLayerCount, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

const layer = ref(0)
/** Editing one layer, or looking at all of them. */
const view = ref<'edit' | 'all'>('edit')
const selected = ref<number | undefined>(props.session.layout.findIndex(hasKey))
watch(() => props.session.layers.length, (count) => (layer.value = Math.min(layer.value, count - 1)))

const actions = computed(() => props.session.layers[layer.value] ?? [])
const keyLegend = (pos: number) => {
  const action = actions.value[pos]
  if (action === undefined) return undefined
  // A see-through key shows, faintly, what it falls through to.
  if (action === 'Transparent') {
    for (let below = layer.value - 1; below >= 0; below--) {
      const lower = props.session.layers[below][pos]
      if (lower !== 'Transparent') return { ...legend(lower), kind: 'through' as const }
    }
  }
  return legend(action)
}

/** The layer whose action is drawn at a position: the first one at or below this layer that is not see-through. */
function shownLayer(pos: number): number {
  for (let at = layer.value; at > 0; at--) {
    if (props.session.layers[at]?.[pos] !== 'Transparent') return at
  }
  return 0
}

// A held key lights up fully only if the keyboard is doing what is drawn on
// it here. Held keys that used another layer, or that the keyboard has not
// acted on (an undecided hold-tap, part of a combo), are marked faintly.
const held = computed(() => {
  const here = new Set<number>()
  const elsewhere = new Set<number>()
  for (const pos of props.session.pressed) {
    const exact = !props.session.layerAware || props.session.usedLayer.get(pos) === shownLayer(pos)
    ;(exact ? here : elsewhere).add(pos)
  }
  return { here, elsewhere }
})
/** The layer new presses are looked up on first. */
const topLayer = computed(() => (props.session.layerAware ? 31 - Math.clz32(props.session.activeLayers | 1) : undefined))
const layerActive = (index: number) => props.session.layerAware && (props.session.activeLayers & (1 << index)) !== 0

/** Knob directions live after this half's switches in the keymap. */
const knobs = computed(() => {
  const count = props.session.hardware.encoders.length
  const first = localPositions(props.session.hardware) - 2 * count
  return [...Array(count).keys()].map((index) => ({ index, ccw: first + 2 * index, cw: first + 2 * index + 1 }))
})
/** Knobs without a place in the picture get buttons under it. */
const looseKnobs = computed(() => knobs.value.filter((knob) => !hasKey(props.session.layout[knob.ccw])))

async function change(action: Action) {
  if (selected.value === undefined) return
  await attempt(() => setAction(props.session, layer.value, selected.value!, action))
}

// --- Layer colours -------------------------------------------------------------
/** A key that leads to a layer wears that layer's colour. */
const keyTint = (pos: number) => {
  const target = keyLegend(pos)?.layer
  return target === undefined ? undefined : layerColor(props.session, target)
}
const coloring = ref(false)
async function pickColor(number: number) {
  coloring.value = false
  await attempt(() => setLayerColor(props.session, layer.value, number))
}
function openLayer(index: number) {
  // A second click on the open layer offers its colours.
  coloring.value = view.value === 'edit' && layer.value === index && !coloring.value
  layer.value = index
  view.value = 'edit'
}

// --- Dragging ------------------------------------------------------------------
/** Something from the picker landed on a key. */
async function onDrop(pos: number, event: DragEvent) {
  const dragged = dropped(event)
  if (!dragged) return
  selected.value = pos
  await attempt(() => setAction(props.session, layer.value, pos, landOn(actions.value[pos], dragged)))
}

/** A key being dragged onto another to swap the two. */
const moving = ref<{ from: number; x: number; y: number; far: boolean }>()
function onDown(pos: number, event: PointerEvent) {
  selected.value = pos
  coloring.value = false
  moving.value = { from: pos, x: event.clientX, y: event.clientY, far: false }
  const start = { x: event.clientX, y: event.clientY }
  const move = (now: PointerEvent) => {
    if (!moving.value) return
    moving.value.x = now.clientX
    moving.value.y = now.clientY
    if (Math.hypot(now.clientX - start.x, now.clientY - start.y) > 8) moving.value.far = true
  }
  window.addEventListener('pointermove', move)
  window.addEventListener(
    'pointerup',
    () => {
      window.removeEventListener('pointermove', move)
      // After the key under the pointer has had its say.
      setTimeout(() => (moving.value = undefined))
    },
    { once: true },
  )
}
async function onUp(pos: number) {
  const drag = moving.value
  moving.value = undefined
  if (!drag?.far || drag.from === pos) return
  const [a, b] = [actions.value[drag.from], actions.value[pos]]
  if (a === undefined || b === undefined) return
  selected.value = pos
  const done = await attempt(async () => {
    await setAction(props.session, layer.value, drag.from, b)
    await setAction(props.session, layer.value, pos, a)
    return true
  })
  if (done) toast('Swapped the two keys')
}

// --- Typing on the computer's own keyboard fills in the keymap ---------------
const TYPING_KEY = 'jmk.keymap.typing'
const typing = ref(localStorage.getItem(TYPING_KEY) !== 'off')
watch(typing, (on) => localStorage.setItem(TYPING_KEY, on ? 'on' : 'off'))

/** A JMK keyboard reported a key change this recently, so what was typed came from it. */
const OWN_KEY_MS = 350
function typedByJmkKeyboard(): boolean {
  const now = performance.now()
  return app.sessions.some((s) => s.transport.kind === 'usb' && !s.closed && (s.pressed.size > 0 || now - s.lastKeyAt < OWN_KEY_MS))
}

/** The key after `pos` in keymap order, or `pos` itself at the end. */
function nextKey(pos: number): number {
  const turns = new Set(knobs.value.flatMap((knob) => [knob.ccw, knob.cw]))
  for (let next = pos + 1; next < props.session.layout.length; next++) {
    // Both directions of a knob count, though only the first has a shape.
    if (hasKey(props.session.layout[next]) || turns.has(next)) return next
  }
  return pos
}

/** Put a typed key on the selected key and move on to the next one. */
function assignTyped(basic: Basic) {
  const pos = selected.value
  if (pos === undefined || typedByJmkKeyboard()) return
  const onLayer = layer.value
  const before = props.session.layers[onLayer]?.[pos]
  if (before === undefined) return
  // The tap half of a hold-tap is the key; its hold half stays.
  const action: Action = typeof before === 'object' && 'HoldTap' in before ? { HoldTap: { ...before.HoldTap, tap: basic } } : { Basic: basic }
  selected.value = nextKey(pos)
  // The keyboard's own report can arrive a moment after what it typed.
  setTimeout(() => {
    if (typedByJmkKeyboard()) {
      if (selected.value === nextKey(pos)) selected.value = pos
      return
    }
    attempt(() => setAction(props.session, onLayer, pos, action))
  }, 60)
}

/** A modifier held on its own; it becomes a key when released untouched. */
let loneModifier: number | undefined

function wantsTyping(event: KeyboardEvent): number | undefined {
  if (!typing.value || selected.value === undefined) return undefined
  const target = event.target as HTMLElement | null
  if (target?.closest?.('input, textarea, select, [contenteditable]')) return undefined
  return usageForCode(event.code)
}

function onKeydown(event: KeyboardEvent) {
  const usage = wantsTyping(event)
  if (usage === undefined) return
  // Cmd/Win shortcuts belong to the app and the system.
  if (event.metaKey && !isModifierUsage(usage)) return
  event.preventDefault()
  if (event.repeat) return
  if (isModifierUsage(usage)) {
    loneModifier = usage
    return
  }
  loneModifier = undefined
  const mods = (event.ctrlKey ? 0x01 : 0) | (event.shiftKey ? 0x02 : 0) | (event.altKey ? 0x04 : 0)
  assignTyped(mods ? { ModKey: { mods, key: usage } } : { Key: usage })
}

function onKeyup(event: KeyboardEvent) {
  const usage = wantsTyping(event)
  if (usage === undefined || usage !== loneModifier) return
  loneModifier = undefined
  assignTyped({ Key: usage })
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown, true)
  window.addEventListener('keyup', onKeyup, true)
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown, true)
  window.removeEventListener('keyup', onKeyup, true)
})

const maxLayers = props.session.info.max_layers
/** One layer change at a time: each builds on the count the last one left. */
const countingLayers = ref(false)
async function changeLayers(by: number) {
  if (countingLayers.value) return
  countingLayers.value = true
  await attempt(() => setLayerCount(props.session, props.session.layers.length + by))
  countingLayers.value = false
  if (by > 0) layer.value = props.session.layers.length - 1
  view.value = 'edit'
}
const addLayer = () => changeLayers(1)
const removeLayer = () => changeLayers(-1)
const selectedAction = computed(() => (selected.value === undefined ? undefined : actions.value[selected.value]))
const missingRemote = computed(() => props.session.layout.length > positionCount(props.session))
</script>

<template>
  <div class="keymap" :class="{ all: view === 'all' }">
    <div class="stage col">
      <div class="layers row" data-test="layers">
        <TransitionGroup name="pop">
          <button
            v-for="(_, index) in session.layers"
            :key="index"
            class="layer"
            :class="{ on: view === 'edit' && layer === index, live: index > 0 && layerActive(index) }"
            :style="{ '--tint': layerColor(session, index) ?? 'var(--muted)' }"
            :data-test="'layer-' + index"
            :title="view === 'edit' && layer === index ? 'Click again to change its colour' : ''"
            @click="openLayer(index)"
          >
            <span class="swatch" :data-test="'layer-swatch-' + index" />
            Layer {{ index }}
            <span v-if="index === 0" class="muted small">base</span>
            <span v-if="index > 0 && layerActive(index)" class="live-dot" title="Active on the keyboard right now" data-test="layer-live" />
          </button>
        </TransitionGroup>
        <button class="btn small" :disabled="countingLayers || session.layers.length >= maxLayers" data-test="add-layer" @click="addLayer">+ layer</button>
        <button v-if="session.layers.length > 1" class="btn small ghost" :disabled="countingLayers" data-test="remove-layer" @click="removeLayer">remove last</button>
        <button class="btn small" :class="{ primary: view === 'all' }" data-test="all-layers" @click="view = view === 'all' ? 'edit' : 'all'">
          {{ view === 'all' ? 'Back to editing' : 'All layers · picture' }}
        </button>
        <span class="grow" />
        <label v-if="view === 'edit'" class="row typing small" title="Press a key on your computer's keyboard to put it on the selected key">
          <input v-model="typing" type="checkbox" data-test="typing" />
          Type to assign
        </label>
      </div>

      <Transition name="rise">
        <div v-if="coloring && view === 'edit'" class="colors row" data-test="layer-colors">
          <span class="muted small">Colour of layer {{ layer }}</span>
          <button
            v-for="(color, index) in LAYER_COLORS"
            :key="color.name"
            class="color"
            :class="{ on: layerColorNumber(session.layerColors, layer) === index + 1 }"
            :style="{ background: color.hex }"
            :title="color.name"
            :data-test="'color-' + (index + 1)"
            @click="pickColor(index + 1)"
          />
          <span v-if="!canColorLayers(session)" class="muted small">Update the firmware under Hardware for the keyboard to remember it.</span>
        </div>
      </Transition>

      <KeymapOverview v-if="view === 'all'" :session="session" />

      <template v-else>
        <div class="board">
          <KeyboardView
            :layout="session.layout"
            :legend="keyLegend"
            :tint="keyTint"
            :pressed="held.here"
            :pressed-elsewhere="held.elsewhere"
            :selected="selected === undefined ? [] : [selected]"
            :knobs="knobPositions(session)"
            interactive
            droppable
            data-test="keymap-board"
            @down="onDown"
            @up="onUp"
            @drop="onDrop"
          />
          <div v-if="looseKnobs.length" class="knobs row">
            <div v-for="knob in looseKnobs" :key="knob.index" class="knob row">
              <button class="turn" :class="{ on: selected === knob.ccw }" :data-test="'knob-' + knob.index + '-ccw'" @click="selected = knob.ccw">
                ⟲ <strong>{{ keyLegend(knob.ccw)?.main || '·' }}</strong>
              </button>
              <span class="dial" />
              <button class="turn" :class="{ on: selected === knob.cw }" :data-test="'knob-' + knob.index + '-cw'" @click="selected = knob.cw">
                <strong>{{ keyLegend(knob.cw)?.main || '·' }}</strong> ⟳
              </button>
            </div>
          </div>
        </div>
        <p class="muted small hint">
          Click a key and pick below, or drag from below onto a key. Drag one key onto another to swap them<template v-if="typing">;
          typing on your computer's keyboard fills the selected key</template>.
          <template v-if="held.elsewhere.size && topLayer !== undefined && topLayer !== layer">
            Layer {{ topLayer }} is active on the keyboard, so the faintly marked keys are doing what that layer says.
          </template>
          <template v-if="missingRemote"> The other half is not connected right now; its keys can still be edited.</template>
        </p>
      </template>
    </div>

    <aside v-if="view === 'edit'" class="side card">
      <ActionPicker
        :action="selectedAction"
        :layer-count="session.layers.length"
        :current-layer="layer"
        :macros="session.macros.map((m) => m.length > 0)"
        :layer-colors="session.layerColors"
        @change="change"
      />
    </aside>

    <div v-if="moving?.far" class="carried" :style="{ left: moving.x + 'px', top: moving.y + 'px' }">
      {{ keyLegend(moving.from)?.main || '·' }}
    </div>
  </div>
</template>

<style scoped>
.keymap {
  display: grid;
  grid-template-rows: minmax(0, 0.8fr) minmax(210px, 1fr);
  gap: 14px;
  height: 100%;
  min-height: 0;
  padding: 14px 24px 16px;
}

.keymap.all {
  grid-template-rows: minmax(0, 1fr);
}

.stage {
  min-height: 0;
  gap: 10px;
}

.layers {
  flex-wrap: wrap;
}

.layer {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 14px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: var(--panel);
  cursor: pointer;
  transition:
    border-color 0.15s,
    background 0.15s,
    transform 0.15s var(--ease);
}

.layer:hover {
  transform: translateY(-1px);
}

.layer.on {
  border-color: var(--tint);
  background: color-mix(in srgb, var(--tint) 16%, var(--panel));
}

.swatch {
  width: 12px;
  height: 12px;
  border-radius: 4px;
  background: var(--tint);
}

.colors {
  gap: 7px;
  flex-wrap: wrap;
}

.color {
  width: 22px;
  height: 22px;
  border-radius: 7px;
  border: 2px solid transparent;
  cursor: pointer;
  transition: transform 0.1s var(--ease);
}

.color:hover {
  transform: scale(1.12);
}

.color.on {
  border-color: var(--text);
}

.carried {
  position: fixed;
  z-index: 50;
  transform: translate(10px, 10px);
  padding: 5px 11px;
  border-radius: 9px;
  background: var(--accent);
  color: var(--accent-ink);
  font-weight: 600;
  pointer-events: none;
  box-shadow: var(--shadow);
}

.typing {
  gap: 6px;
  color: var(--muted);
  cursor: pointer;
}

.typing input {
  accent-color: var(--accent);
}

.layer.live {
  border-color: var(--signal);
}

.live-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--signal);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--signal) 25%, transparent);
}

.small {
  font-size: 0.82rem;
}

.board {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 4px 10px;
}

/* The drawing shrinks to the room there is, in both directions. */
.board > svg {
  flex: 1 1 0;
  min-height: 0;
}

.knob {
  gap: 6px;
}

.turn {
  padding: 7px 12px;
  border-radius: 10px;
  border: 2px solid transparent;
  background: var(--cap-top);
  cursor: pointer;
}

.turn.on {
  border-color: var(--accent);
}

.dial {
  width: 34px;
  height: 34px;
  border-radius: 50%;
  background: conic-gradient(from 20deg, var(--cap), var(--cap-top), var(--cap));
  border: 2px solid var(--line);
}

.hint {
  text-align: center;
}

.side {
  min-height: 0;
  padding: 14px 16px;
}
</style>
