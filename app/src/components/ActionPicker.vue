<script setup lang="ts">
// Chooses what a key does. One bar says what the selected key is now; tabs
// hold an ordinary keyboard to pick from and, each on its own page, everything
// a keyboard does not have. Click to put something on the selected keys, or
// drag it onto any key.
import { computed, ref, watch } from 'vue'
import { startDrag, type Dragged } from '../lib/drag'
import { ANSI_BLOCKS, boardBlocks, describeAction, describeBasic, EXTRA_KEYS, KEY_GROUPS, keyIcon, keyLabel, keyName, MEDIA_KEYS, MOD_BITS, modKeyChar } from '../lib/keycodes'
import { ALTGR_BIT, language, languageId, LANGUAGES, symbols } from '../lib/language'
import { layerHex } from '../lib/layers'
import type { Action, Basic } from '../lib/protocol'
import { macroLabel, type Session } from '../lib/session'
import MacroEditor from './MacroEditor.vue'

const props = defineProps<{
  /** The selected key, if any. */
  action?: Action
  layerCount: number
  currentLayer: number
  /** The keyboard, for its macros: they are named, recorded and edited right here. */
  session: Session
  layerColors?: number[]
  /** How many keys are selected; what is picked goes on all of them. */
  count?: number
  /** Whether the selected keys do different things. */
  mixed?: boolean
  /** Folded down to its top bar, so the keyboard has the room. */
  minimized?: boolean
}>()

const emit = defineEmits<{ change: [action: Action]; minimize: [on: boolean] }>()

const search = ref('')
// Looking for a key means wanting to see what is found.
watch(search, (text) => text && props.minimized && emit('minimize', false))
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
/** The macro on the selected key, if it plays one. */
const chosenMacro = computed(() => (typeof current.value === 'object' && 'Macro' in current.value ? current.value.Macro : undefined))
/** The macro shown in the editor beside the list: the one on the key, or the one clicked last. */
const openMacro = ref(chosenMacro.value ?? 0)
watch(chosenMacro, (now) => now !== undefined && (openMacro.value = now))
function pickMacro(index: number) {
  openMacro.value = index
  choose({ Macro: index })
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
  ...KEY_GROUPS.flatMap((group) => group.keys).map((key) => ({ label: keyLabel(key.usage), name: keyName(key.usage), basic: { Key: key.usage } as Basic })),
  ...MEDIA_KEYS.map((key) => ({ label: key.label, name: key.name, basic: { Consumer: key.usage } as Basic })),
  ...MOD_BITS.slice(0, 4).map((mod) => ({ label: `${mod.label} once`, name: `${mod.label} for the next key only`, basic: { OneShotMods: mod.bit } as Basic })),
])
const found = computed(() => {
  const query = search.value.trim().toLowerCase()
  if (!query) return undefined
  return everything.value.filter((item) => item.name.toLowerCase().includes(query) || item.label.toLowerCase() === query)
})
const foundBasic = (item: Found): Basic => (typeof item.basic === 'object' && 'Key' in item.basic ? keyBasic(item.basic.Key) : item.basic)

const PLATFORMS = [
  { id: 'pc', name: 'Windows and Linux' },
  { id: 'mac', name: 'macOS' },
] as const

// --- Pages --------------------------------------------------------------------
const TABS = [
  { id: 'keys', name: 'Keyboard' },
  { id: 'symbols', name: 'Symbols' },
  { id: 'layers', name: 'Layers' },
  { id: 'mods', name: 'One-shot' },
  { id: 'media', name: 'Media' },
  { id: 'macros', name: 'Macros' },
  { id: 'more', name: 'More' },
] as const
type TabId = (typeof TABS)[number]['id']
const tab = ref<TabId>('keys')

const ON_BOARD = new Set(ANSI_BLOCKS.flatMap((block) => block.rows.flat()).map(([usage]) => usage))
/** The page that holds what the selected key does now, marked with a dot. */
const home = computed<TabId | undefined>(() => {
  const basic = current.value
  if (props.action === 'Transparent' || basic === 'None') return 'more'
  if (!basic || props.mixed) return undefined
  if ('Key' in basic) return ON_BOARD.has(basic.Key) ? 'keys' : 'more'
  if ('ModKey' in basic) return modKeyChar(basic.ModKey.mods, basic.ModKey.key) ? 'symbols' : ON_BOARD.has(basic.ModKey.key) ? 'keys' : 'more'
  if ('MomentaryLayer' in basic || 'ToggleLayer' in basic || 'ToLayer' in basic) return 'layers'
  if ('OneShotMods' in basic) return 'mods'
  if ('Consumer' in basic) return 'media'
  if ('Macro' in basic) return 'macros'
  return undefined
})

const summary = computed(() => {
  if (props.action === undefined) return 'Click a key above'
  if ((props.count ?? 1) > 1 && props.mixed) return 'different things'
  if (chosenMacro.value !== undefined && !holdTap.value && props.session.macroNames[chosenMacro.value]) return `Macro: ${macroLabel(props.session, chosenMacro.value)}`
  return describeAction(props.action)
})
</script>

<template>
  <div class="picker" :class="{ minimized }">
    <div class="bar row">
      <div class="now">
        <span class="muted small" data-test="picker-target">{{ action === undefined ? 'No key selected' : (count ?? 1) > 1 ? `${count} keys` : 'This key' }}</span>
        <strong data-test="action-description" :title="summary">{{ summary }}</strong>
      </div>

      <template v-if="action !== undefined && !minimized">
        <label class="row toggle" title="For example a letter when tapped and Shift when held">
          <input type="checkbox" :checked="!!holdTap" data-test="hold-tap" @change="toggleHoldTap" />
          <span>Different when held</span>
        </label>
        <div v-if="holdTap" class="slots row">
          <button class="slot" :class="{ on: slot === 'tap' }" data-test="slot-tap" @click="slot = 'tap'">
            <span class="muted small">tapped</span>{{ describeBasic(holdTap.tap) }}
          </button>
          <button class="slot" :class="{ on: slot === 'hold' }" data-test="slot-hold" @click="slot = 'hold'">
            <span class="muted small">held</span>{{ describeBasic(holdTap.hold) }}
          </button>
          <label class="row feel muted small" title="Which of the four hold-tap feels under Behaviours this key uses">feel
            <select class="input" :value="holdTap.profile" @change="emit('change', { HoldTap: { ...holdTap, profile: Number(($event.target as HTMLSelectElement).value) } })">
              <option v-for="n in 4" :key="n" :value="n - 1">{{ n }}</option>
            </select>
          </label>
        </div>
      </template>

      <span class="grow" />
      <select v-model="languageId" class="input language" title="The language your computer's keyboard setting is on, and the kind of computer. It changes what the keys are called here, not what the keyboard sends." aria-label="Language of the computer" data-test="language" @change="($event.target as HTMLElement).blur()">
        <optgroup v-for="group in PLATFORMS" :key="group.id" :label="group.name">
          <option v-for="each in LANGUAGES.filter((one) => one.platform === group.id)" :key="each.id" :value="each.id">{{ each.name }}</option>
        </optgroup>
      </select>
      <input v-model="search" class="input search" placeholder="Search keys…" data-test="search" />
      <button class="fold" :title="minimized ? 'Open the key picker' : 'Minimise, so the keyboard has the room'" :aria-label="minimized ? 'Open the key picker' : 'Minimise the key picker'" :aria-expanded="!minimized" data-test="picker-fold" @click="emit('minimize', !minimized)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="minimized ? 'm18 15-6-6-6 6' : 'm6 9 6 6 6-6'" /></svg>
      </button>
    </div>

    <div v-show="!minimized" class="tabs row">
      <button v-for="t in TABS" :key="t.id" class="tab" :class="{ on: !found && tab === t.id }" :data-test="'tab-' + t.id" @click="((tab = t.id), (search = ''))">
        {{ t.name }}<span v-if="home === t.id" class="here" />
      </button>
      <span class="grow" />
      <div v-if="found || tab === 'keys' || tab === 'more'" class="mods row" title="Send these together with the key you pick, e.g. Ctrl with C">
        <span class="muted small">together with</span>
        <button v-for="mod in MOD_BITS.slice(0, 4)" :key="mod.bit" class="chip-btn" :class="{ on: withMods & mod.bit }" :data-test="'with-' + mod.label" @click="withMods ^= mod.bit">
          {{ mod.label }}
        </button>
        <button class="chip-btn" :class="{ on: withMods & ALTGR_BIT }" title="The right Alt key, which many languages use for a third character on a key" data-test="with-AltGr" @click="withMods ^= ALTGR_BIT">{{ language().platform === 'mac' ? 'ROption' : 'AltGr' }}</button>
      </div>
    </div>

    <div v-show="!minimized" class="pane">
      <section v-if="found" class="page scroll">
        <p v-if="!found.length" class="muted">Nothing found.</p>
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

      <div v-else-if="tab === 'keys'" class="board">
        <div v-for="block in boardBlocks()" :key="block.id" class="block" :class="block.id">
          <div v-for="(keys, index) in block.rows" :key="index" class="keys">
            <template v-for="([usage, width], at) in keys" :key="at">
              <span v-if="usage === undefined" class="gap" :style="{ '--w': width }" />
              <button
                v-else
                class="key-btn cap"
                :class="{ on: isKey(usage), long: !keyIcon(usage) && keyLabel(usage).length > 4 && width < 1.5 }"
                :style="{ '--w': width }"
                :title="keyName(usage)"
                :data-usage="usage"
                draggable="true"
                @dragstart="drag($event, { basic: keyBasic(usage) })"
                @click="choose(keyBasic(usage))"
              >
                <template v-if="keyIcon(usage)">
                  <span class="icon" aria-hidden="true">{{ keyIcon(usage) }}</span>
                  <span class="word" :class="{ 'visually-hidden': width < 1.5 }">{{ keyLabel(usage) }}</span>
                </template>
                <template v-else>{{ keyLabel(usage) }}</template>
              </button>
            </template>
          </div>
        </div>
      </div>

      <section v-else-if="tab === 'layers'" class="page scroll">
        <p class="muted small">A key that takes you to another layer: only while you hold it, switched on and off with a tap, or for good until you switch back.</p>
        <p v-if="!otherLayers.length" class="muted">There is only one layer so far. Add another above the keyboard first.</p>
        <div class="layer-rows">
          <div v-for="layer in otherLayers" :key="layer" class="layer-row" :style="layerStyle(layer)">
            <span class="layer-name"><span class="dot" />Layer {{ layer }}</span>
            <button class="key-btn tint" :class="{ on: same({ MomentaryLayer: layer }) }" :data-test="'layer-hold-' + layer" draggable="true" @dragstart="drag($event, { basic: { MomentaryLayer: layer } })" @click="choose({ MomentaryLayer: layer })">while held</button>
            <button class="key-btn tint" :class="{ on: same({ ToggleLayer: layer }) }" :data-test="'layer-toggle-' + layer" draggable="true" @dragstart="drag($event, { basic: { ToggleLayer: layer } })" @click="choose({ ToggleLayer: layer })">on / off</button>
            <button class="key-btn tint" :class="{ on: same({ ToLayer: layer }) }" :data-test="'layer-to-' + layer" draggable="true" @dragstart="drag($event, { basic: { ToLayer: layer } })" @click="choose({ ToLayer: layer })">switch to</button>
          </div>
        </div>
      </section>

      <section v-else-if="tab === 'mods'" class="page scroll">
        <p class="muted small">Tap one of these and only the next key gets that modifier, so nothing has to be held. Ordinary Shift, Ctrl and so on are on the Keyboard page.</p>
        <div class="grid big">
          <button v-for="mod in MOD_BITS.slice(0, 4)" :key="mod.bit" class="key-btn" :class="{ on: same({ OneShotMods: mod.bit }) }" :data-test="'oneshot-' + mod.label" draggable="true" @dragstart="drag($event, { basic: { OneShotMods: mod.bit } })" @click="choose({ OneShotMods: mod.bit })">
            {{ mod.label }} once
          </button>
        </div>
      </section>

      <section v-else-if="tab === 'symbols'" class="page scroll">
        <p class="muted small">Characters that need Shift or {{ language().platform === 'mac' ? 'Option' : 'AltGr' }} on a {{ language().platform === 'mac' ? 'Mac' : 'Windows or Linux computer' }} set to {{ language().name }}. The key holds the modifier for you.</p>
        <div class="grid">
          <button
            v-for="each in symbols()"
            :key="each.char"
            class="key-btn symbol"
            :class="{ on: same({ ModKey: { mods: each.mods, key: each.usage } }) }"
            :title="describeBasic({ ModKey: { mods: each.mods, key: each.usage } })"
            :data-symbol="each.char"
            draggable="true"
            @dragstart="drag($event, { basic: { ModKey: { mods: each.mods, key: each.usage } } })"
            @click="choose({ ModKey: { mods: each.mods, key: each.usage } })"
          >
            {{ each.char }}
          </button>
        </div>
      </section>

      <section v-else-if="tab === 'media'" class="page scroll">
        <div class="grid big">
          <button v-for="key in MEDIA_KEYS" :key="key.usage" class="key-btn" :class="{ on: same({ Consumer: key.usage }) }" :title="key.name" :data-test="'media-' + key.usage" draggable="true" @dragstart="drag($event, { basic: { Consumer: key.usage } })" @click="choose({ Consumer: key.usage })">
            {{ key.name }}
          </button>
        </div>
      </section>

      <section v-else-if="tab === 'macros'" class="macros">
        <div class="macro-list scroll">
          <button
            v-for="(steps, index) in session.macros"
            :key="index"
            class="macro-item"
            :class="{ on: same({ Macro: index }), open: openMacro === index, faint: !steps.length }"
            :title="`Put ${macroLabel(session, index)} on the key`"
            :data-test="'macro-' + index"
            draggable="true"
            @dragstart="drag($event, { basic: { Macro: index } })"
            @click="pickMacro(index)"
          >
            <span class="num">M{{ index + 1 }}</span>
            <span class="nm">{{ session.macroNames[index] || (steps.length ? 'unnamed' : 'empty') }}</span>
          </button>
        </div>
        <div class="macro-edit scroll">
          <MacroEditor :session="session" :index="openMacro" />
        </div>
      </section>

      <section v-else class="page scroll">
        <div class="grid big">
          <button class="key-btn" :class="{ on: same('None') }" title="The key does nothing" data-test="none" draggable="true" @dragstart="drag($event, { basic: 'None' })" @click="choose('None')">Nothing</button>
          <button class="key-btn" :class="{ on: action === 'Transparent' }" title="Uses whatever this key does on the layer below" data-test="transparent" draggable="true" @dragstart="drag($event, { action: 'Transparent' })" @click="action !== undefined && emit('change', 'Transparent')">
            See-through
          </button>
        </div>
        <p class="muted small">See-through uses whatever the key does on the layer below.</p>
        <div class="grid">
          <button v-for="key in EXTRA_KEYS" :key="key.usage" class="key-btn" :class="{ on: isKey(key.usage) }" :title="keyName(key.usage)" :data-usage="key.usage" draggable="true" @dragstart="drag($event, { basic: keyBasic(key.usage) })" @click="choose(keyBasic(key.usage))">
            {{ keyLabel(key.usage) }}
          </button>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.picker {
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
}

.now {
  display: flex;
  flex-direction: column;
  min-width: 110px;
  max-width: 300px;
  line-height: 1.25;
}

.now strong {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.toggle {
  gap: 7px;
  cursor: pointer;
  font-size: 0.9rem;
  white-space: nowrap;
}

.toggle input {
  accent-color: var(--accent);
}

.slots {
  gap: 6px;
}

.slot {
  display: flex;
  align-items: baseline;
  gap: 7px;
  padding: 5px 11px;
  border-radius: 9px;
  border: 1px solid var(--line);
  background: var(--panel-2);
  cursor: pointer;
  white-space: nowrap;
  transition: border-color 0.15s;
}

.slot.on {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.feel {
  gap: 6px;
}

.feel .input {
  width: 52px;
  padding: 4px 6px;
}

.search {
  width: 200px;
  flex: none;
}

.cap .icon {
  font-size: 1.15em;
  line-height: 1;
}

.cap .word {
  margin-left: 0.35em;
  font-size: 0.8em;
  opacity: 0.55;
}

.cap .word.visually-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}

.key-btn.symbol {
  min-width: 36px;
  font-size: 1rem;
}

.language {
  flex: none;
  width: auto;
  max-width: 170px;
}

.fold {
  flex: none;
  width: 32px;
  height: 32px;
  margin-left: -6px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--muted);
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}

.fold:hover {
  background: var(--panel-2);
  color: var(--text);
}

.fold svg {
  width: 17px;
  height: 17px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.picker.minimized {
  height: auto;
}

.picker.minimized .bar {
  min-height: 0;
}

.picker.minimized .now {
  flex-direction: row;
  align-items: baseline;
  gap: 8px;
  max-width: 420px;
}

.tabs {
  gap: 2px;
  border-bottom: 1px solid var(--line);
}

.tab {
  position: relative;
  padding: 6px 12px 8px;
  border: none;
  border-bottom: 2px solid transparent;
  margin-bottom: -1px;
  background: none;
  color: var(--muted);
  cursor: pointer;
  transition: color 0.15s;
}

.tab:hover {
  color: var(--text);
}

.tab.on {
  color: var(--text);
  font-weight: 600;
  border-bottom-color: var(--accent);
}

.here {
  display: inline-block;
  width: 6px;
  height: 6px;
  margin-left: 6px;
  border-radius: 50%;
  background: var(--accent);
  vertical-align: middle;
}

.mods {
  gap: 5px;
  padding-bottom: 5px;
}

.chip-btn {
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

/* The page takes the room that is left; the keyboard is sized to fill it. */
.pane {
  flex: 1;
  min-height: 0;
  container-type: size;
}

.page {
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 2px 4px 2px 0;
}

.page p {
  margin: 0;
  max-width: 640px;
}

.macros {
  height: 100%;
  display: grid;
  grid-template-columns: 190px minmax(0, 1fr);
  gap: 16px;
}

.macro-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
  padding-right: 4px;
}

.macro-edit {
  min-height: 0;
  padding-left: 16px;
  border-left: 1px solid var(--line);
}

.macro-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 9px;
  border-radius: 8px;
  border: 1px solid transparent;
  background: none;
  text-align: left;
  cursor: grab;
  transition:
    background 0.15s,
    border-color 0.15s;
}

.macro-item:hover {
  background: var(--panel-2);
}

.macro-item.open {
  background: var(--panel-2);
  border-color: var(--line);
}

.macro-item.on {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.macro-item .num {
  width: 28px;
  flex: none;
  color: var(--muted);
  font-size: 0.78rem;
}

.macro-item .nm {
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.macro-item.faint .nm {
  font-weight: 400;
  color: var(--muted);
}

/* The ordinary keyboard. One key unit is --u wide. */
.board {
  --u: clamp(20px, min((100cqw - 34px) / 22, 100cqh / 6), 50px);
  height: 100%;
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 16px;
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
  font-size: calc(var(--u) * 0.33);
}

.key-btn.cap.long {
  font-size: calc(var(--u) * 0.25);
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

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(56px, 1fr));
  gap: 5px;
}

.grid.big {
  grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
}

.grid .key-btn {
  min-height: 38px;
}

.key-btn.wide {
  grid-column: span 3;
}

.layer-rows {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
  gap: 6px 26px;
}

.layer-row {
  display: grid;
  grid-template-columns: 78px repeat(3, 1fr);
  gap: 5px;
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

</style>
