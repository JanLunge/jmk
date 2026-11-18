<script setup lang="ts">
// Chooses what one key does: an ordinary keyboard to pick from, with
// everything a keyboard does not have grouped below it. Click to put
// something on the selected key, or drag it onto any key.
import { computed, ref, watch } from 'vue'
import { startDrag, type Dragged } from '../lib/drag'
import { ANSI_BLOCKS, describeAction, describeBasic, EXTRA_KEYS, KEY_GROUPS, keyLabel, keyName, MEDIA_KEYS, MOD_BITS } from '../lib/keycodes'
import { layerHex } from '../lib/layers'
import type { Action, Basic } from '../lib/protocol'

const props = defineProps<{
  /** The selected key, if any. */
  action?: Action
  layerCount: number
  currentLayer: number
  /** Which macros have steps, by index. */
  macros: boolean[]
  layerColors?: number[]
}>()

const emit = defineEmits<{ change: [action: Action] }>()

const search = ref('')
/** Modifiers to send together with the next key picked. */
const withMods = ref(0)
/** For a hold-tap: which of its two actions is being chosen. */
const slot = ref<'tap' | 'hold'>('tap')

const holdTap = computed(() => (typeof props.action === 'object' && 'HoldTap' in props.action ? props.action.HoldTap : undefined))
watch(holdTap, (now) => !now && (slot.value = 'tap'))

/** Put `basic` where the user is pointing: the whole key, or one half of a hold-tap. */
function choose(basic: Basic) {
  if (props.action === undefined) return
  if (holdTap.value) emit('change', { HoldTap: { ...holdTap.value, [slot.value]: basic } })
  else emit('change', { Basic: basic })
}

const keyBasic = (usage: number): Basic => (withMods.value ? { ModKey: { mods: withMods.value, key: usage } } : { Key: usage })

function toggleHoldTap() {
  if (holdTap.value) {
    emit('change', { Basic: holdTap.value.tap })
    return
  }
  const tap: Basic = typeof props.action === 'object' && 'Basic' in props.action ? props.action.Basic : 'None'
  emit('change', { HoldTap: { tap, hold: { Mods: 0x02 }, profile: 0 } })
  slot.value = 'hold'
}

const current = computed<Basic | undefined>(() => {
  if (holdTap.value) return holdTap.value[slot.value]
  return typeof props.action === 'object' && 'Basic' in props.action ? props.action.Basic : undefined
})
const isKey = (usage: number) => {
  const basic = current.value
  if (!basic || basic === 'None') return false
  return ('Key' in basic && basic.Key === usage) || ('ModKey' in basic && basic.ModKey.key === usage)
}
const same = (basic: Basic) => JSON.stringify(current.value) === JSON.stringify(basic)
const otherLayers = computed(() => [...Array(props.layerCount).keys()].filter((layer) => layer !== props.currentLayer))
const layerStyle = (layer: number) => {
  const hex = layerHex(props.layerColors ?? [], layer)
  return hex ? { '--tint': hex } : undefined
}

const drag = (event: DragEvent, dragged: Dragged) => startDrag(event, dragged)

// --- Search ------------------------------------------------------------------
interface Found {
  label: string
  name: string
  basic: Basic
}
const everything = computed<Found[]>(() => [
  ...KEY_GROUPS.flatMap((group) => group.keys).map((key) => ({ label: key.label, name: key.name, basic: { Key: key.usage } as Basic })),
  ...MEDIA_KEYS.map((key) => ({ label: key.label, name: key.name, basic: { Consumer: key.usage } as Basic })),
  ...MOD_BITS.slice(0, 4).map((mod) => ({ label: `${mod.label} once`, name: `${mod.label} for the next key only`, basic: { OneShotMods: mod.bit } as Basic })),
])
const found = computed(() => {
  const query = search.value.trim().toLowerCase()
  if (!query) return undefined
  return everything.value.filter((item) => item.name.toLowerCase().includes(query) || item.label.toLowerCase() === query)
})
const foundBasic = (item: Found): Basic => (typeof item.basic === 'object' && 'Key' in item.basic ? keyBasic(item.basic.Key) : item.basic)

// --- Jumping to a group --------------------------------------------------------
const GROUPS = [
  { id: 'keys', name: 'Keyboard' },
  { id: 'layers', name: 'Layers' },
  { id: 'mods', name: 'Modifiers' },
  { id: 'media', name: 'Media' },
  { id: 'macros', name: 'Macros' },
  { id: 'more', name: 'More' },
] as const
const pane = ref<HTMLElement>()
function jump(id: string) {
  search.value = ''
  // After the search results have made way for the groups again.
  requestAnimationFrame(() => pane.value?.querySelector(`[data-group=${id}]`)?.scrollIntoView({ block: 'start', behavior: 'smooth' }))
}
</script>

<template>
  <div class="picker">
    <div class="side col">
      <div class="now">
        <div class="muted small">{{ action === undefined ? 'No key selected' : 'This key' }}</div>
        <strong data-test="action-description">{{ action === undefined ? 'Click a key above, or drag something onto one' : describeAction(action) }}</strong>
      </div>

      <template v-if="action !== undefined">
        <label class="row toggle">
          <input type="checkbox" :checked="!!holdTap" data-test="hold-tap" @change="toggleHoldTap" />
          <span>Do something different when held</span>
        </label>
        <div v-if="holdTap" class="slots row">
          <button class="slot" :class="{ on: slot === 'tap' }" data-test="slot-tap" @click="slot = 'tap'">
            <span class="muted small">tapped</span>{{ describeBasic(holdTap.tap) }}
          </button>
          <button class="slot" :class="{ on: slot === 'hold' }" data-test="slot-hold" @click="slot = 'hold'">
            <span class="muted small">held</span>{{ describeBasic(holdTap.hold) }}
          </button>
          <label class="field profile">feel
            <select class="input" :value="holdTap.profile" @change="emit('change', { HoldTap: { ...holdTap, profile: Number(($event.target as HTMLSelectElement).value) } })">
              <option v-for="n in 4" :key="n" :value="n - 1">{{ n }}</option>
            </select>
          </label>
        </div>
      </template>

      <input v-model="search" class="input search" placeholder="Search keys…" data-test="search" />
      <div class="mods row">
        <span class="muted small">with</span>
        <button v-for="mod in MOD_BITS.slice(0, 4)" :key="mod.bit" class="chip-btn" :class="{ on: withMods & mod.bit }" :data-test="'with-' + mod.label" @click="withMods ^= mod.bit">
          {{ mod.label }}
        </button>
      </div>
      <div class="jumps row">
        <button v-for="group in GROUPS" :key="group.id" class="jump" :data-test="'tab-' + group.id" @click="jump(group.id)">{{ group.name }}</button>
      </div>
    </div>

    <div ref="pane" class="pane scroll">
      <section v-if="found" class="group">
        <h3 class="muted">{{ found.length ? 'Found' : 'Nothing found' }}</h3>
        <div class="grid">
          <button
            v-for="item in found"
            :key="item.name"
            class="key-btn wide"
            :class="{ on: same(foundBasic(item)) }"
            :title="item.name"
            draggable="true"
            @dragstart="drag($event, { basic: foundBasic(item) })"
            @click="choose(foundBasic(item))"
          >
            {{ item.name }}
          </button>
        </div>
      </section>

      <template v-else>
        <div class="board" data-group="keys">
          <div v-for="block in ANSI_BLOCKS" :key="block.id" class="block" :class="block.id">
            <div v-for="(keys, index) in block.rows" :key="index" class="keys">
              <template v-for="([usage, width], at) in keys" :key="at">
                <span v-if="usage === undefined" class="gap" :style="{ '--w': width }" />
                <button
                  v-else
                  class="key-btn cap"
                  :class="{ on: isKey(usage), long: keyLabel(usage).length > 4 && width < 1.5 }"
                  :style="{ '--w': width }"
                  :title="keyName(usage)"
                  :data-usage="usage"
                  draggable="true"
                  @dragstart="drag($event, { basic: keyBasic(usage) })"
                  @click="choose(keyBasic(usage))"
                >
                  {{ keyLabel(usage) }}
                </button>
              </template>
            </div>
          </div>
        </div>

        <div class="groups">
          <section class="group" data-group="layers">
            <h3 class="muted">Layers</h3>
            <p v-if="!otherLayers.length" class="muted small">There is only one layer so far. Add another above the keyboard, then put a key here that takes you there.</p>
            <div v-for="layer in otherLayers" :key="layer" class="layer-row" :style="layerStyle(layer)">
              <span class="layer-name"><span class="dot" />Layer {{ layer }}</span>
              <button class="key-btn tint" :class="{ on: same({ MomentaryLayer: layer }) }" :data-test="'layer-hold-' + layer" draggable="true" @dragstart="drag($event, { basic: { MomentaryLayer: layer } })" @click="choose({ MomentaryLayer: layer })">while held</button>
              <button class="key-btn tint" :class="{ on: same({ ToggleLayer: layer }) }" :data-test="'layer-toggle-' + layer" draggable="true" @dragstart="drag($event, { basic: { ToggleLayer: layer } })" @click="choose({ ToggleLayer: layer })">on / off</button>
              <button class="key-btn tint" :class="{ on: same({ ToLayer: layer }) }" :data-test="'layer-to-' + layer" draggable="true" @dragstart="drag($event, { basic: { ToLayer: layer } })" @click="choose({ ToLayer: layer })">switch to</button>
            </div>
          </section>

          <section class="group" data-group="mods">
            <h3 class="muted">Modifiers for the next key only</h3>
            <div class="grid">
              <button v-for="mod in MOD_BITS.slice(0, 4)" :key="mod.bit" class="key-btn" :class="{ on: same({ OneShotMods: mod.bit }) }" :data-test="'oneshot-' + mod.label" draggable="true" @dragstart="drag($event, { basic: { OneShotMods: mod.bit } })" @click="choose({ OneShotMods: mod.bit })">
                {{ mod.label }}
              </button>
            </div>
            <p class="muted small">Tap it, then the next key gets that modifier. Ordinary Shift, Ctrl and so on are on the keyboard above.</p>
          </section>

          <section class="group" data-group="media">
            <h3 class="muted">Media</h3>
            <div class="grid">
              <button v-for="key in MEDIA_KEYS" :key="key.usage" class="key-btn" :class="{ on: same({ Consumer: key.usage }) }" :title="key.name" :data-test="'media-' + key.usage" draggable="true" @dragstart="drag($event, { basic: { Consumer: key.usage } })" @click="choose({ Consumer: key.usage })">
                {{ key.label }}
              </button>
            </div>
          </section>

          <section class="group" data-group="macros">
            <h3 class="muted">Macros</h3>
            <div class="grid">
              <button v-for="(filled, index) in macros" :key="index" class="key-btn" :class="{ on: same({ Macro: index }), faint: !filled }" :title="filled ? `Macro ${index + 1}` : `Macro ${index + 1} (empty)`" :data-test="'macro-' + index" draggable="true" @dragstart="drag($event, { basic: { Macro: index } })" @click="choose({ Macro: index })">
                M{{ index + 1 }}
              </button>
            </div>
            <p class="muted small">A macro types a whole sequence with one press. Write them under Behaviours.</p>
          </section>

          <section class="group" data-group="more">
            <h3 class="muted">More keys</h3>
            <div class="grid">
              <button v-for="key in EXTRA_KEYS" :key="key.usage" class="key-btn" :class="{ on: isKey(key.usage) }" :title="key.name" :data-usage="key.usage" draggable="true" @dragstart="drag($event, { basic: keyBasic(key.usage) })" @click="choose(keyBasic(key.usage))">
                {{ key.label }}
              </button>
            </div>
          </section>

          <section class="group" data-group="special">
            <h3 class="muted">Special</h3>
            <div class="grid">
              <button class="key-btn wide" :class="{ on: same('None') }" data-test="none" draggable="true" @dragstart="drag($event, { basic: 'None' })" @click="choose('None')">Nothing</button>
              <button class="key-btn wide" :class="{ on: action === 'Transparent' }" data-test="transparent" draggable="true" @dragstart="drag($event, { action: 'Transparent' })" @click="action !== undefined && emit('change', 'Transparent')">
                See-through
              </button>
            </div>
            <p class="muted small">See-through uses whatever this key does on the layer below.</p>
          </section>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.picker {
  display: grid;
  grid-template-columns: 250px minmax(0, 1fr);
  gap: 18px;
  height: 100%;
  min-height: 0;
}

.side {
  min-height: 0;
  gap: 10px;
  overflow-y: auto;
}

.small {
  font-size: 0.82rem;
}

.toggle {
  cursor: pointer;
  font-size: 0.92rem;
}

.toggle input {
  accent-color: var(--accent);
}

.slots {
  align-items: stretch;
  flex-wrap: wrap;
  gap: 6px;
}

.slot {
  flex: 1;
  min-width: 90px;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  text-align: left;
  padding: 6px 10px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: var(--panel-2);
  cursor: pointer;
  transition: border-color 0.15s;
}

.slot.on {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.profile {
  width: 58px;
}

.search {
  width: 100%;
}

.mods,
.jumps {
  gap: 5px;
  flex-wrap: wrap;
}

.chip-btn,
.jump {
  padding: 3px 9px;
  border-radius: 999px;
  border: 1px solid var(--line);
  background: var(--panel-2);
  font-size: 0.82rem;
  cursor: pointer;
  transition:
    border-color 0.15s,
    color 0.15s;
}

.chip-btn.on {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-ink);
  font-weight: 600;
}

.jump {
  color: var(--muted);
  background: none;
}

.jump:hover {
  color: var(--text);
  border-color: var(--muted);
}

.pane {
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding-right: 4px;
}

/* The ordinary keyboard. One key unit is --u wide. */
.board {
  --u: 36px;
  display: flex;
  flex-wrap: wrap;
  gap: 4px 16px;
}

.block {
  display: flex;
  flex-direction: column;
}

.keys {
  display: flex;
  height: var(--u);
}

.gap {
  width: calc(var(--u) * var(--w));
  flex: none;
}

.key-btn {
  min-height: 32px;
  padding: 3px 6px;
  border-radius: 7px;
  border: 1px solid var(--line);
  background: var(--panel-2);
  cursor: grab;
  font-size: 0.84rem;
  white-space: nowrap;
  overflow: hidden;
  transition:
    transform 0.1s var(--ease),
    border-color 0.15s,
    background 0.15s;
}

.key-btn.cap {
  flex: none;
  width: calc(var(--u) * var(--w) - 3px);
  height: calc(var(--u) - 3px);
  min-height: 0;
  padding: 0 2px;
  font-size: 0.78rem;
}

.key-btn.cap.long {
  font-size: 0.62rem;
  letter-spacing: -0.02em;
}

.key-btn:hover {
  border-color: var(--muted);
  transform: translateY(-1px);
}

.key-btn:active {
  transform: scale(0.95);
}

.key-btn.on {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-ink);
  font-weight: 600;
}

.key-btn.faint:not(.on) {
  opacity: 0.5;
}

.groups {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(290px, 1fr));
  gap: 16px 22px;
  align-items: start;
}

.group {
  display: flex;
  flex-direction: column;
  gap: 7px;
  scroll-margin-top: 4px;
}

h3 {
  font-size: 0.76rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(46px, 1fr));
  gap: 4px;
}

.key-btn.wide {
  grid-column: span 3;
}

.layer-row {
  display: grid;
  grid-template-columns: 78px repeat(3, 1fr);
  gap: 4px;
  align-items: center;
}

.layer-name {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.86rem;
}

.dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--tint, var(--muted));
}

.key-btn.tint:not(.on) {
  background: color-mix(in srgb, var(--tint, transparent) 22%, var(--panel-2));
  border-color: color-mix(in srgb, var(--tint, var(--line)) 50%, var(--line));
}

@media (max-width: 860px) {
  .picker {
    grid-template-columns: 1fr;
    grid-template-rows: auto minmax(0, 1fr);
  }
}
</style>
