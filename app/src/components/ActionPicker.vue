<script setup lang="ts">
// Chooses what one key does.
import { computed, ref, watch } from 'vue'
import { describeAction, describeBasic, KEY_GROUPS, MEDIA_KEYS, MOD_BITS } from '../lib/keycodes'
import type { Action, Basic } from '../lib/protocol'

const props = defineProps<{
  action: Action
  layerCount: number
  currentLayer: number
  /** Names of the macros that have steps, by index. */
  macros: boolean[]
}>()

const emit = defineEmits<{ change: [action: Action] }>()

type Tab = 'keys' | 'layers' | 'media' | 'macros' | 'more'
const tab = ref<Tab>('keys')
const search = ref('')
/** Modifiers to send together with the next key picked. */
const withMods = ref(0)
/** For a hold-tap: which of its two actions is being chosen. */
const slot = ref<'tap' | 'hold'>('tap')

const holdTap = computed(() => (typeof props.action === 'object' && 'HoldTap' in props.action ? props.action.HoldTap : undefined))
watch(holdTap, (now) => !now && (slot.value = 'tap'))

const groups = computed(() => {
  const query = search.value.trim().toLowerCase()
  if (!query) return KEY_GROUPS
  return KEY_GROUPS.map((group) => ({
    ...group,
    keys: group.keys.filter((key) => key.name.toLowerCase().includes(query) || key.label.toLowerCase() === query),
  })).filter((group) => group.keys.length)
})

/** Put `basic` where the user is pointing: the whole key, or one half of a hold-tap. */
function choose(basic: Basic) {
  if (holdTap.value) emit('change', { HoldTap: { ...holdTap.value, [slot.value]: basic } })
  else emit('change', { Basic: basic })
}

function chooseKey(usage: number) {
  choose(withMods.value ? { ModKey: { mods: withMods.value, key: usage } } : { Key: usage })
}

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
</script>

<template>
  <div class="picker col">
    <div class="now">
      <div class="muted small">This key</div>
      <strong data-test="action-description">{{ describeAction(action) }}</strong>
    </div>

    <label class="row toggle">
      <input type="checkbox" :checked="!!holdTap" data-test="hold-tap" @change="toggleHoldTap" />
      <span>Do something different when held</span>
    </label>
    <Transition name="rise">
      <div v-if="holdTap" class="slots row">
        <button class="slot" :class="{ on: slot === 'tap' }" data-test="slot-tap" @click="slot = 'tap'">
          <span class="muted small">tapped</span>{{ describeBasic(holdTap.tap) }}
        </button>
        <button class="slot" :class="{ on: slot === 'hold' }" data-test="slot-hold" @click="slot = 'hold'">
          <span class="muted small">held</span>{{ describeBasic(holdTap.hold) }}
        </button>
        <label class="field profile">feel
          <select
            class="input"
            :value="holdTap.profile"
            @change="emit('change', { HoldTap: { ...holdTap, profile: Number(($event.target as HTMLSelectElement).value) } })"
          >
            <option v-for="n in 4" :key="n" :value="n - 1">{{ n }}</option>
          </select>
        </label>
      </div>
    </Transition>

    <div class="tabs row">
      <button v-for="t in ['keys', 'layers', 'media', 'macros', 'more'] as Tab[]" :key="t" class="tab" :class="{ on: tab === t }" :data-test="'tab-' + t" @click="tab = t">
        {{ t }}
      </button>
    </div>

    <div class="pane scroll">
      <template v-if="tab === 'keys'">
        <input v-model="search" class="input search" placeholder="Search keys…" data-test="search" />
        <div class="mods row">
          <span class="muted small">with</span>
          <button
            v-for="mod in MOD_BITS.slice(0, 4)"
            :key="mod.bit"
            class="key-btn mod"
            :class="{ on: withMods & mod.bit }"
            :data-test="'with-' + mod.label"
            @click="withMods ^= mod.bit"
          >
            {{ mod.label }}
          </button>
        </div>
        <section v-for="group in groups" :key="group.title">
          <h3 class="muted">{{ group.title }}</h3>
          <div class="grid">
            <button
              v-for="key in group.keys"
              :key="key.usage"
              class="key-btn"
              :class="{ on: isKey(key.usage) }"
              :title="key.name"
              :data-usage="key.usage"
              @click="chooseKey(key.usage)"
            >
              {{ key.label }}
            </button>
          </div>
        </section>
      </template>

      <template v-else-if="tab === 'layers'">
        <p class="muted small">
          A layer is a second keyboard on the same keys, like Shift gives you capitals. Add layers above the keyboard,
          then put a key here that takes you there.
        </p>
        <p v-if="!otherLayers.length" class="note">There is only one layer so far. Add another one first.</p>
        <section v-for="layer in otherLayers" :key="layer">
          <h3 class="muted">Layer {{ layer }}</h3>
          <div class="grid wide">
            <button class="key-btn" :class="{ on: same({ MomentaryLayer: layer }) }" :data-test="'layer-hold-' + layer" @click="choose({ MomentaryLayer: layer })">while held</button>
            <button class="key-btn" :class="{ on: same({ ToggleLayer: layer }) }" @click="choose({ ToggleLayer: layer })">toggle on/off</button>
            <button class="key-btn" :class="{ on: same({ ToLayer: layer }) }" @click="choose({ ToLayer: layer })">switch to it</button>
          </div>
        </section>
      </template>

      <template v-else-if="tab === 'media'">
        <div class="grid wide">
          <button
            v-for="key in MEDIA_KEYS"
            :key="key.usage"
            class="key-btn"
            :class="{ on: same({ Consumer: key.usage }) }"
            :data-test="'media-' + key.usage"
            @click="choose({ Consumer: key.usage })"
          >
            {{ key.name }}
          </button>
        </div>
      </template>

      <template v-else-if="tab === 'macros'">
        <p class="muted small">A macro types a whole sequence with one press. Write them under Behaviours.</p>
        <div class="grid">
          <button
            v-for="(filled, index) in macros"
            :key="index"
            class="key-btn"
            :class="{ on: same({ Macro: index }), faint: !filled }"
            :data-test="'macro-' + index"
            @click="choose({ Macro: index })"
          >
            M{{ index + 1 }}
          </button>
        </div>
      </template>

      <template v-else>
        <section>
          <h3 class="muted">Modifiers alone</h3>
          <div class="grid wide">
            <button v-for="mod in MOD_BITS" :key="mod.bit" class="key-btn" :class="{ on: same({ Key: 0xe0 + MOD_BITS.indexOf(mod) }) }" @click="choose({ Key: 0xe0 + MOD_BITS.indexOf(mod) })">
              {{ mod.label }}
            </button>
          </div>
        </section>
        <section>
          <h3 class="muted">For the next key only (one-shot)</h3>
          <div class="grid wide">
            <button v-for="mod in MOD_BITS.slice(0, 4)" :key="mod.bit" class="key-btn" :class="{ on: same({ OneShotMods: mod.bit }) }" @click="choose({ OneShotMods: mod.bit })">
              {{ mod.label }}
            </button>
          </div>
        </section>
        <section>
          <h3 class="muted">Special</h3>
          <div class="grid wide">
            <button class="key-btn" :class="{ on: same('None') }" data-test="none" @click="choose('None')">Nothing</button>
            <button class="key-btn" :class="{ on: action === 'Transparent' }" data-test="transparent" @click="emit('change', 'Transparent')">
              See-through
            </button>
          </div>
          <p class="muted small">See-through uses whatever this key does on the layer below.</p>
        </section>
      </template>
    </div>
  </div>
</template>

<style scoped>
.picker {
  height: 100%;
  min-height: 0;
  gap: 12px;
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
}

.slot {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  text-align: left;
  padding: 7px 11px;
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

.tabs {
  gap: 2px;
  border-bottom: 1px solid var(--line);
}

.tab {
  padding: 7px 11px;
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  color: var(--muted);
  cursor: pointer;
  text-transform: capitalize;
  transition: color 0.15s;
}

.tab:hover {
  color: var(--text);
}

.tab.on {
  color: var(--text);
  border-bottom-color: var(--accent);
}

.pane {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding-right: 4px;
}

.search {
  width: 100%;
}

.mods {
  gap: 6px;
}

section {
  display: flex;
  flex-direction: column;
  gap: 7px;
}

h3 {
  font-size: 0.78rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(44px, 1fr));
  gap: 5px;
}

.grid.wide {
  grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
}

.key-btn {
  min-height: 36px;
  padding: 4px 6px;
  border-radius: 8px;
  border: 1px solid var(--line);
  background: var(--panel-2);
  cursor: pointer;
  font-size: 0.86rem;
  transition:
    transform 0.1s var(--ease),
    border-color 0.15s,
    background 0.15s;
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
</style>
