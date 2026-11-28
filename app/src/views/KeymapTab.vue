<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import ActionPicker from '../components/ActionPicker.vue'
import BottomSheet from '../components/BottomSheet.vue'
import KeyboardView from '../components/KeyboardView.vue'
import KeymapOverview from '../components/KeymapOverview.vue'
import LayoutEditor from '../components/LayoutEditor.vue'
import LetterRows from '../components/LetterRows.vue'
import { app, attempt, toast } from '../lib/app'
import { localPositions } from '../lib/definition'
import { dropped, landOn } from '../lib/drag'
import { isModifierUsage, legend, usageForCode } from '../lib/keycodes'
import { LAYER_COLORS, layerColor, layerColorNumber, onWith } from '../lib/layers'
import { hasKey, nextInRow } from '../lib/layout'
import type { Action, Basic } from '../lib/protocol'
import { recordingKeys } from '../lib/recording'
import { ALPHAS, guessAlphas, letterActions } from '../lib/starter'
import { canColorLayers, canJoinLayers, canKeepLetterRows, knobPositions, positionCount, setAction, setLayerColor, setLayerCount, setLayerWith, setLetterRows, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

const layer = ref(0)
/** Editing one layer's keys, arranging the picture of the keyboard, or looking at all layers. */
const view = ref<'edit' | 'layout' | 'all'>('edit')
/** The layout tools while they are open. The keyboard is then drawn from their draft, and a press on a key goes to them. */
const arranger = ref<InstanceType<typeof LayoutEditor>>()
const VIEWS = [
  { id: 'edit', name: 'Keys', test: 'view-keys', title: 'Choose what each key does', icon: 'M6 5h12a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2ZM9 15l3-6 3 6M10 13h4' },
  { id: 'layout', name: 'Layout', test: 'edit-layout', title: 'Move, size and turn the keys so the picture matches your keyboard', icon: 'M12 3v18M3 12h18M12 3 9.5 5.5M12 3l2.5 2.5M12 21l-2.5-2.5M12 21l2.5-2.5M3 12l2.5-2.5M3 12l2.5 2.5M21 12l-2.5-2.5M21 12l-2.5 2.5' },
  { id: 'all', name: 'All layers', test: 'all-layers', title: 'See every layer side by side', icon: 'm12 4 9 4.5-9 4.5-9-4.5ZM3 13l9 4.5 9-4.5M3 17l9 4.5' },
] as const
/** The keys being edited. What is picked goes on all of them; the last one is the one shown. */
const selection = ref<number[]>([props.session.layout.findIndex(hasKey)].filter((pos) => pos >= 0))
const selected = computed<number | undefined>({
  get: () => selection.value[selection.value.length - 1],
  set: (pos) => (selection.value = pos === undefined ? [] : [pos]),
})
watch(() => props.session.layers.length, (count) => (layer.value = Math.min(layer.value, count - 1)))

const actions = computed(() => props.session.layers[layer.value] ?? [])
/** The layers a see-through key of the open layer can fall to, highest first. */
const under = computed(() => onWith(props.session.layersWith, layer.value).slice(1))
const keyLegend = (pos: number) => {
  const action = actions.value[pos]
  if (action === undefined) return undefined
  // A see-through key shows, faintly, what it falls through to.
  if (action === 'Transparent') {
    for (const below of under.value) {
      const lower = props.session.layers[below][pos]
      if (lower !== 'Transparent') return { ...legend(lower), kind: 'through' as const }
    }
  }
  return named(action, legend(action))
}
/** While arranging, every key also says which position of the keymap it is. */
const boardLegend = (pos: number) => (arranger.value && actions.value[pos] !== undefined ? { ...keyLegend(pos)!, sub: String(pos) } : keyLegend(pos))

/** A key that plays a named macro shows the name, as far as it fits. */
function named(action: Action, drawn: ReturnType<typeof legend>) {
  const basic = typeof action === 'object' && 'Basic' in action ? action.Basic : undefined
  const index = typeof basic === 'object' && 'Macro' in basic ? basic.Macro : undefined
  const name = index === undefined ? '' : (props.session.macroNames[index] ?? '')
  if (!name) return drawn
  return { ...drawn, main: name.length > 7 ? name.slice(0, 6) + '…' : name, sub: `M${index! + 1}` }
}

/** The layer whose action is drawn at a position: this one, or the first one a see-through key falls to that has a key. */
function shownLayer(pos: number): number {
  return [layer.value, ...under.value].find((at) => props.session.layers[at]?.[pos] !== 'Transparent') ?? 0
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
  const keys = [...selection.value]
  await attempt(async () => {
    for (const pos of keys) await setAction(props.session, layer.value, pos, action)
  })
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
/** The layers that turn the open one on while they are on together, one bit each. */
const joined = computed(() => props.session.layersWith[layer.value] ?? 0)
const toggleJoined = (other: number) => attempt(() => setLayerWith(props.session, layer.value, joined.value ^ (1 << other)))
function openLayer(index: number) {
  // A second click on the open layer offers its colours and what turns it on.
  coloring.value = view.value === 'edit' && layer.value === index && !coloring.value
  layer.value = index
  if (view.value === 'all') view.value = 'edit'
}

// --- Dragging ------------------------------------------------------------------
/** Something from the picker landed on a key. */
async function onDrop(pos: number, event: DragEvent) {
  const dragged = dropped(event)
  if (!dragged) return
  // Dropped on one of several selected keys, it goes on all of them.
  if (!selection.value.includes(pos) || selection.value.length < 2) selected.value = pos
  const keys = [...selection.value]
  await attempt(async () => {
    for (const key of keys) await setAction(props.session, layer.value, key, landOn(actions.value[key], dragged))
  })
}

/** A key being dragged onto another to swap the two. */
const moving = ref<{ from: number; x: number; y: number; far: boolean }>()
function onDown(pos: number, event: PointerEvent) {
  coloring.value = false
  if (arranger.value) return arranger.value.down(pos, event)
  // With Shift, Cmd or Ctrl a key joins or leaves the selection.
  if (event.shiftKey || event.metaKey || event.ctrlKey) {
    selection.value = selection.value.includes(pos) ? selection.value.filter((key) => key !== pos) : [...selection.value, pos]
    return
  }
  selected.value = pos
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
const PICKER_KEY = 'jmk.keymap.picker'
/** The sheet under the keyboard folded down to one bar; kept, as it is a way of working and not a passing state. */
const minimized = ref(localStorage.getItem(PICKER_KEY) === 'min')
watch(minimized, (on) => localStorage.setItem(PICKER_KEY, on ? 'min' : 'open'))
const TYPING_KEY = 'jmk.keymap.typing'
// Off unless asked for: a key press meant for something else must not change the keymap.
const typing = ref(localStorage.getItem(TYPING_KEY) === 'on')
watch(typing, (on) => localStorage.setItem(TYPING_KEY, on ? 'on' : 'off'))

/** A JMK keyboard reported a key change this recently, so what was typed came from it. */
const OWN_KEY_MS = 350
function typedByJmkKeyboard(): boolean {
  const now = performance.now()
  return app.sessions.some((s) => s.transport.kind === 'usb' && !s.closed && (s.pressed.size > 0 || now - s.lastKeyAt < OWN_KEY_MS))
}

/**
 * The key after `pos`: the next one to the right, else the first of the row
 * below, as text is read. `pos` itself after the last key.
 */
function nextKey(pos: number): number {
  const layout = props.session.layout
  // A knob is turned both ways: the second direction comes right after the first, and has no shape of its own.
  const knob = knobs.value.find((k) => k.ccw === pos || k.cw === pos)
  if (knob?.ccw === pos) return knob.cw
  const from = knob ? knob.ccw : pos
  if (hasKey(layout[from])) return nextInRow(layout, from) ?? pos
  // Without a place in the picture, keymap order is all there is.
  for (let next = pos + 1; next < layout.length; next++) {
    if (hasKey(layout[next]) || knobs.value.some((k) => k.ccw === next || k.cw === next)) return next
  }
  return pos
}

/** Put a typed key on the selected key and move on to the next one. With several selected it goes on all of them, and they stay selected. */
function assignTyped(basic: Basic) {
  const keys = [...selection.value]
  const pos = selected.value
  if (pos === undefined || typedByJmkKeyboard()) return
  const onLayer = layer.value
  const typed = keys.flatMap((key) => {
    const before = props.session.layers[onLayer]?.[key]
    if (before === undefined) return []
    // The tap half of a hold-tap is the key; its hold half stays.
    const action: Action = typeof before === 'object' && 'HoldTap' in before ? { HoldTap: { ...before.HoldTap, tap: basic } } : { Basic: basic }
    return [{ key, action }]
  })
  if (!typed.length) return
  if (keys.length === 1) selected.value = nextKey(pos)
  // The keyboard's own report can arrive a moment after what it typed.
  setTimeout(() => {
    if (typedByJmkKeyboard()) {
      if (keys.length === 1 && selected.value === nextKey(pos)) selected.value = pos
      return
    }
    attempt(async () => {
      for (const { key, action } of typed) await setAction(props.session, onLayer, key, action)
    })
  }, 60)
}

/** A modifier held on its own; it becomes a key when released untouched. */
let loneModifier: number | undefined

function wantsTyping(event: KeyboardEvent): number | undefined {
  if (view.value !== 'edit' || !typing.value || recordingKeys.value || lettersOpen.value || selected.value === undefined) return undefined
  const target = event.target as HTMLElement | null
  if (target?.closest?.('input, textarea, select, [contenteditable]')) return undefined
  return usageForCode(event.code)
}

function onKeydown(event: KeyboardEvent) {
  const usage = wantsTyping(event)
  if (usage === undefined) return
  // Cmd/Win shortcuts belong to the app and the system, and so do undo and
  // redo with Ctrl. The modifier held for them is not a key being typed.
  const undoKey = event.ctrlKey && (event.code === 'KeyZ' || event.code === 'KeyY')
  if ((event.metaKey || undoKey) && !isModifierUsage(usage)) {
    loneModifier = undefined
    return
  }
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
// --- Letters -------------------------------------------------------------------
// The whole block of letters at once: where its rows are is remembered, so
// another arrangement is a matter of picking it.
const lettersOpen = ref(false)
const letterRowsDraft = ref<number[][]>([[], [], []])
const alphasDraft = ref(ALPHAS[0].id)
function openLetters() {
  letterRowsDraft.value = [0, 1, 2].map((row) => [...(props.session.letterRows[row] ?? [])])
  alphasDraft.value = guessAlphas(props.session.layers[layer.value], letterRowsDraft.value).id
  lettersOpen.value = true
}
async function applyLetters() {
  const rows = letterRowsDraft.value
  const alphas = ALPHAS.find((a) => a.id === alphasDraft.value) ?? ALPHAS[0]
  const onLayer = layer.value
  lettersOpen.value = false
  const done = await attempt(async () => {
    await setLetterRows(props.session, rows)
    for (const [pos, action] of letterActions(rows, alphas)) {
      const before = props.session.layers[onLayer]?.[pos]
      if (before === undefined) continue
      // What a key does when held stays; only the letter it types changes.
      const next: Action = typeof before === 'object' && 'HoldTap' in before && typeof action === 'object' && 'Basic' in action ? { HoldTap: { ...before.HoldTap, tap: action.Basic } } : action
      if (JSON.stringify(next) !== JSON.stringify(before)) await setAction(props.session, onLayer, pos, next)
    }
    return true
  })
  if (done) toast(`${alphas.name} is on layer ${onLayer}`)
}

/**
 * A keyboard big enough for the alphabet that types no letter yet: point at
 * the quick way to place them. A macro pad has no use for it.
 */
const wantsLetters = computed(() => {
  const knobs = knobPositions(props.session)
  const switches = props.session.layout.filter((key, pos) => hasKey(key) && !knobs.includes(pos)).length
  if (switches < 26) return false
  const usages = [...JSON.stringify(props.session.layers).matchAll(/"Key":(\d+)/g)].map((match) => Number(match[1]))
  return !usages.some((usage) => usage >= 0x04 && usage <= 0x1d)
})

const layerElsewhere = computed(() => held.value.elsewhere.size > 0 && topLayer.value !== undefined && topLayer.value !== layer.value)
const selectedAction = computed(() => (selected.value === undefined ? undefined : actions.value[selected.value]))
const mixed = computed(() => new Set(selection.value.map((pos) => JSON.stringify(actions.value[pos]))).size > 1)

// --- Selecting several keys by dragging across them -----------------------------
const board = ref<HTMLElement>()
const marquee = ref<{ x: number; y: number; w: number; h: number }>()
function bandDown(event: PointerEvent) {
  if (event.button !== 0 || (event.target as Element).closest('[data-pos], button')) return
  const start = { x: event.clientX, y: event.clientY }
  const kept = event.shiftKey ? [...selection.value] : []
  const move = (now: PointerEvent) => {
    const box = { x: Math.min(start.x, now.clientX), y: Math.min(start.y, now.clientY), w: Math.abs(now.clientX - start.x), h: Math.abs(now.clientY - start.y) }
    if (!marquee.value && box.w + box.h < 6) return
    marquee.value = box
    const inside = [...(board.value?.querySelectorAll<SVGElement>('[data-pos]') ?? [])]
      .filter((key) => {
        const r = key.getBoundingClientRect()
        return r.left < box.x + box.w && r.right > box.x && r.top < box.y + box.h && r.bottom > box.y
      })
      .map((key) => Number(key.dataset.pos))
    selection.value = [...new Set([...kept, ...inside])]
  }
  window.addEventListener('pointermove', move)
  window.addEventListener(
    'pointerup',
    () => {
      window.removeEventListener('pointermove', move)
      // A plain click beside the keys lets go of the selection.
      if (!marquee.value && !event.shiftKey) selection.value = []
      marquee.value = undefined
    },
    { once: true },
  )
}
const missingRemote = computed(() => props.session.layout.length > positionCount(props.session))
</script>

<template>
  <div class="keymap" :class="{ all: view === 'all', minimized }">
    <div class="stage col">
      <div class="layers row" data-test="layers">
        <TransitionGroup name="pop">
          <button
            v-for="(_, index) in session.layers"
            :key="index"
            class="layer"
            :class="{ on: view !== 'all' && layer === index, live: index > 0 && layerActive(index) }"
            :style="{ '--tint': layerColor(session, index) ?? 'var(--muted)' }"
            :data-test="'layer-' + index"
            :title="view === 'edit' && layer === index ? 'Click again to change its colour or what turns it on' : ''"
            @click="openLayer(index)"
          >
            <span class="swatch" :data-test="'layer-swatch-' + index" />
            Layer {{ index }}
            <span v-if="index === 0" class="muted small">base</span>
            <span v-if="index > 0 && layerActive(index)" class="live-dot" title="Active on the keyboard right now" data-test="layer-live" />
          </button>
        </TransitionGroup>
        <button class="btn small ghost" title="Add a layer" :disabled="countingLayers || session.layers.length >= maxLayers" data-test="add-layer" @click="addLayer">+</button>
        <button v-if="session.layers.length > 1" class="btn small ghost" title="Remove the last layer" :disabled="countingLayers" data-test="remove-layer" @click="removeLayer">−</button>
        <span class="grow" />
        <!-- Helpers for filling in the keys; they only make sense while doing that. -->
        <template v-if="view === 'edit'">
          <button class="tool" title="Put QWERTY, Colemak or Dvorak on this layer in one go" data-test="letters-open" @click="openLetters">
            <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="6" width="18" height="12" rx="3" /><path d="M7.5 10h.01M12 10h.01M16.5 10h.01M8 14h8" /></svg>
            Place letters
          </button>
          <!-- Let go of the switch once flipped: the next key press is meant for the keymap, not for the switch. -->
          <button
            class="tool"
            :class="{ on: typing }"
            role="switch"
            :aria-checked="typing"
            title="Press a key on your computer's keyboard to put it on the selected key, and move on to the next"
            data-test="typing"
            @click="((typing = !typing), ($event.currentTarget as HTMLElement).blur())"
          >
            <span class="track" aria-hidden="true" />
            Type to assign
          </button>
        </template>
        <!-- What the keyboard below is for right now. Always in the same place, so it is also the way back. -->
        <div class="views" role="tablist" aria-label="What to do with the keyboard">
          <button v-for="each in VIEWS" :key="each.id" class="view" :class="{ on: view === each.id }" role="tab" :aria-selected="view === each.id" :title="each.title" :data-test="each.test" @click="((view = each.id), (coloring = false))">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="each.icon" /></svg>
            {{ each.name }}
          </button>
        </div>
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
          <template v-if="layer > 0 && session.layers.length > 3 && canJoinLayers(session)">
            <span class="muted small joined" title="For a third layer reached with two layer keys: it is on for as long as all the layers chosen here are">Also on while these are on together</span>
            <template v-for="(_, other) in session.layers" :key="other">
              <button v-if="other > 0 && other !== layer" class="btn small" :class="{ primary: joined & (1 << other) }" :data-test="'join-' + other" @click="toggleJoined(other)">
                Layer {{ other }}
              </button>
            </template>
          </template>
        </div>
      </Transition>

      <KeymapOverview v-if="view === 'all'" :session="session" />

      <template v-else>
        <!-- One keyboard for both: arranging it puts it on a grid, in a box, and nothing moves. -->
        <div ref="board" class="board" :class="{ boxed: arranger }" :title="arranger ? '' : 'Drag a key onto another to swap them. Shift-click or drag across keys to change several at once.'" @pointerdown="bandDown">
          <KeyboardView
            :layout="arranger?.draft ?? session.layout"
            :frame="arranger?.frame"
            :grid="!!arranger"
            :legend="boardLegend"
            :tint="keyTint"
            :pressed="held.here"
            :pressed-elsewhere="held.elsewhere"
            :selected="arranger?.selected ?? selection"
            :knobs="knobPositions(session)"
            :max-unit="minimized ? 84 : 62"
            interactive
            :droppable="!arranger"
            data-test="keymap-board"
            @down="onDown"
            @up="onUp"
            @drop="onDrop"
          />
          <div v-if="looseKnobs.length" class="knobs row" :class="{ away: arranger }">
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
        <!-- The room for these is always kept: a line that comes and goes would make the keyboard jump. -->
        <div class="small hints">
          <span v-if="view === 'layout'" class="muted" data-test="layout-hint">Drag keys to where they sit on your keyboard. Drag across the grid to select several; arrow keys nudge, R turns.</span>
          <span v-else-if="wantsLetters" class="nudge" data-test="letters-nudge">
            No letters on this keyboard yet.
            <button class="btn small primary" data-test="letters-nudge-open" @click="openLetters">Place the letters</button>
            <span class="muted">QWERTY, Colemak or Dvorak in one go</span>
          </span>
          <span v-if="view === 'edit' && (layerElsewhere || missingRemote)" class="muted" data-test="keymap-hint">
            <template v-if="layerElsewhere">Layer {{ topLayer }} is active on the keyboard, so the faintly marked keys are doing what that layer says.</template>
            <template v-if="missingRemote"> The other half is not connected right now; its keys can still be edited.</template>
          </span>
        </div>
      </template>
    </div>

    <LayoutEditor v-if="view === 'layout'" ref="arranger" v-model:selection="selection" v-model:minimized="minimized" :session="session" />
    <BottomSheet v-else-if="view === 'edit'" v-model:minimized="minimized" data-test="picker">
      <ActionPicker
        :action="selectedAction"
        :layer-count="session.layers.length"
        :current-layer="layer"
        :session="session"
        :layer-colors="session.layerColors"
        :count="selection.length"
        :mixed="mixed"
        :minimized="minimized"
        @change="change"
        @minimize="minimized = $event"
      />
    </BottomSheet>

    <div v-if="marquee" class="marquee" :style="{ left: marquee.x + 'px', top: marquee.y + 'px', width: marquee.w + 'px', height: marquee.h + 'px' }" data-test="keymap-marquee" />
    <div v-if="moving?.far" class="carried" :style="{ left: moving.x + 'px', top: moving.y + 'px' }">
      {{ keyLegend(moving.from)?.main || '·' }}
    </div>
    <Transition name="fade">
      <div v-if="lettersOpen" class="overlay" @click.self="lettersOpen = false">
        <div class="card col letters-dialog" data-test="letters-dialog">
          <div>
            <h2>Letters on layer {{ layer }}</h2>
            <p class="muted small">
              Only the letter keys change; what a key does when held stays.
              <template v-if="!canKeepLetterRows(session)">This keyboard's firmware does not remember the rows yet: update it under Hardware.</template>
            </p>
          </div>
          <LetterRows v-model:rows="letterRowsDraft" v-model:alphas="alphasDraft" :layout="session.layout" :knobs="knobPositions(session)" />
          <div class="row">
            <span class="grow" />
            <button class="btn ghost" @click="lettersOpen = false">Cancel</button>
            <button class="btn primary" :disabled="!letterRowsDraft.some((row) => row.length)" data-test="letters-apply" @click="applyLetters">
              Put {{ ALPHAS.find((a) => a.id === alphasDraft)?.name }} on layer {{ layer }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.keymap {
  display: grid;
  /* As wide as the page, however much the sheet holds. */
  grid-template-columns: minmax(0, 1fr);
  grid-template-rows: minmax(150px, 1fr) minmax(230px, min(54%, 440px));
  gap: 14px;
  height: 100%;
  min-height: 0;
  padding: 14px 24px 16px;
}

.keymap.all {
  grid-template-rows: minmax(0, 1fr);
}

.keymap.minimized:not(.all) {
  grid-template-rows: minmax(150px, 1fr) auto;
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

.joined {
  margin-left: 14px;
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

.marquee {
  position: fixed;
  z-index: 40;
  border: 1px solid var(--accent);
  border-radius: 4px;
  background: var(--accent-soft);
  pointer-events: none;
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

.tool,
.view {
  display: flex;
  align-items: center;
  gap: 7px;
  height: 34px;
  font-size: 0.87rem;
  white-space: nowrap;
  cursor: pointer;
  transition:
    border-color 0.15s,
    background 0.15s,
    color 0.15s;
}

/* Only their own icons: the page around this tab is a .view as well. */
.tool > svg,
.views svg {
  width: 16px;
  height: 16px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.7;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.tool {
  padding: 0 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel);
}

.tool:hover {
  border-color: var(--muted);
}

.tool.on {
  border-color: var(--accent);
  background: var(--accent-soft);
}

/* A small switch: off to the left, on to the right. */
.track {
  width: 26px;
  height: 15px;
  padding: 2px;
  border-radius: 999px;
  background: var(--line);
  transition: background 0.15s;
}

.track::after {
  content: '';
  display: block;
  width: 11px;
  height: 11px;
  border-radius: 50%;
  background: var(--text);
  transition: transform 0.15s var(--ease);
}

.on .track {
  background: var(--accent);
}

.on .track::after {
  transform: translateX(11px);
  background: var(--accent-ink);
}

/* One control with three positions, like the undo and save group above it. */
.views {
  display: flex;
  margin-left: 8px;
  padding: 3px;
  gap: 2px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--panel);
}

.view {
  height: 28px;
  padding: 0 11px;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--muted);
}

.view:hover {
  color: var(--text);
}

.view.on {
  background: var(--accent);
  color: var(--accent-ink);
  font-weight: 600;
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
  user-select: none;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 4px 10px;
  border: 1px solid transparent;
  border-radius: var(--radius);
  transition:
    background 0.2s,
    border-color 0.2s;
}

/* Arranging: the grid under the keys runs to the edge of this box. */
.board.boxed {
  overflow: hidden;
  border-color: var(--line);
  background: var(--panel);
}

.board.boxed :deep(.key),
.board.boxed :deep(.half) {
  cursor: grab;
}

/* Knobs with no place in the picture keep their room, so the keyboard stays put. */
.knobs.away {
  visibility: hidden;
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

.hints {
  flex: none;
  height: 30px;
  overflow: hidden;
}

.hints,
.nudge {
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 10px;
}

.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  display: grid;
  place-items: center;
  z-index: 40;
}

.letters-dialog {
  width: min(880px, 94vw);
  max-height: 92vh;
  overflow: auto;
  gap: 14px;
  box-shadow: var(--shadow);
}
</style>
