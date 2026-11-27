<script setup lang="ts">
// The timing-based parts of a keymap: hold-tap feel, combos and macros.
import { computed, ref } from 'vue'
import ActionPicker from '../components/ActionPicker.vue'
import KeyboardView from '../components/KeyboardView.vue'
import MacroEditor from '../components/MacroEditor.vue'
import { attempt } from '../lib/app'
import { describeAction, legend } from '../lib/keycodes'
import type { Action, Combo, HoldTapFlavor, HoldTapProfile } from '../lib/protocol'
import { LIMITS } from '../lib/protocol'
import { canTuneCombos, knobPositions, setCombos, setProfile, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()
type Section = 'hold-tap' | 'combos' | 'macros'
const section = ref<Section>('hold-tap')

// --- Hold-tap ----------------------------------------------------------------
const FLAVORS: { id: HoldTapFlavor; name: string; text: string }[] = [
  { id: 'TapPreferred', name: 'Patient', text: 'Only counts as held after the waiting time. Other keys never matter. Best when you rarely hold.' },
  { id: 'Balanced', name: 'Balanced', text: 'Also counts as held if you press and release another key while it is down. A good default for home-row modifiers.' },
  { id: 'HoldPreferred', name: 'Eager', text: 'Counts as held as soon as any other key goes down. Best for a thumb layer key.' },
]
const updateProfile = (index: number, change: Partial<HoldTapProfile>) =>
  attempt(() => setProfile(props.session, index, { ...props.session.profiles[index], ...change }))

// --- Combos ------------------------------------------------------------------
const comboIndex = ref(0)
const combo = computed<Combo | undefined>(() => props.session.combos[comboIndex.value])
const saveCombos = (combos: Combo[]) => attempt(() => setCombos(props.session, combos))

async function addCombo() {
  await saveCombos([...props.session.combos, { keys: [], action: { Basic: { Key: 0x29 } }, timeout_ms: 50 }])
  comboIndex.value = props.session.combos.length - 1
}

function patchCombo(change: Partial<Combo>) {
  const combos = props.session.combos.map((c, i) => {
    if (i !== comboIndex.value) return c
    // What is not set is left out, so a plain combo reads as it always did.
    const { layers, sequence, ...rest } = { ...c, ...change }
    return { ...rest, ...(layers ? { layers } : {}), ...(sequence ? { sequence } : {}) }
  })
  return saveCombos(combos)
}

/** Sequences, chosen layers and more than four keys need firmware that knows them. */
const tunable = computed(() => canTuneCombos(props.session))
const keyLimit = computed(() => (tunable.value ? LIMITS.comboKeys : LIMITS.comboKeysBefore))

function toggleComboKey(pos: number) {
  if (!combo.value) return
  const keys = combo.value.keys.includes(pos) ? combo.value.keys.filter((k) => k !== pos) : [...combo.value.keys, pos]
  if (keys.length <= keyLimit.value) patchCombo({ keys })
}

// A chord is quick; the keys of a sequence each get time of their own.
const CHORD_MS = { min: 20, max: 150, step: 5, start: 50 }
const SEQUENCE_MS = { min: 100, max: 2000, step: 50, start: 500 }
const timing = computed(() => (combo.value?.sequence ? SEQUENCE_MS : CHORD_MS))

function setSequence(sequence: boolean) {
  if (!combo.value || !!combo.value.sequence === sequence) return
  const range = sequence ? SEQUENCE_MS : CHORD_MS
  const fits = combo.value.timeout_ms >= range.min && combo.value.timeout_ms <= range.max
  patchCombo({ sequence, timeout_ms: fits ? combo.value.timeout_ms : range.start })
}

const onLayer = (layer: number) => !!((combo.value?.layers ?? 0) & (1 << layer))
const toggleComboLayer = (layer: number) => patchCombo({ layers: (combo.value?.layers ?? 0) ^ (1 << layer) })
const comboName = (c: Combo) => c.keys.map((k) => baseLegend(k)?.main || k).join(c.sequence ? ' → ' : ' + ')

async function removeCombo() {
  await saveCombos(props.session.combos.filter((_, i) => i !== comboIndex.value))
  comboIndex.value = Math.max(0, comboIndex.value - 1)
}

const baseLegend = (pos: number) => (props.session.layers[0]?.[pos] ? legend(props.session.layers[0][pos]) : undefined)

// --- Macros ------------------------------------------------------------------
const macroIndex = ref(0)
</script>

<template>
  <div class="behaviours">
    <nav class="col sections">
      <button v-for="s in ['hold-tap', 'combos', 'macros'] as Section[]" :key="s" class="section" :class="{ on: section === s }" :data-test="'section-' + s" @click="section = s">
        {{ { 'hold-tap': 'Hold and tap', combos: 'Combos', macros: 'Macros' }[s] }}
      </button>
    </nav>

    <Transition name="rise" mode="out-in">
      <!-- Hold-tap ------------------------------------------------------------->
      <div v-if="section === 'hold-tap'" key="hold-tap" class="pane scroll col">
        <div class="intro">
          <h2>One key, two jobs</h2>
          <p class="muted">
            A key can do one thing when tapped and another when held, like <em>A</em> and <em>Shift</em>. The keyboard
            has to guess which you meant. These four "feels" tune that guess; each hold-tap key picks one.
          </p>
        </div>
        <div class="profiles">
          <div v-for="(profile, index) in session.profiles" :key="index" class="card col">
            <h3>Feel {{ index + 1 }}</h3>
            <div class="col flavors">
              <label v-for="flavor in FLAVORS" :key="flavor.id" class="flavor" :class="{ on: profile.flavor === flavor.id }">
                <input type="radio" :name="'flavor' + index" :checked="profile.flavor === flavor.id" :data-test="`flavor-${index}-${flavor.id}`" @change="updateProfile(index, { flavor: flavor.id })" />
                <div><strong>{{ flavor.name }}</strong><div class="muted small">{{ flavor.text }}</div></div>
              </label>
            </div>
            <label class="field">Counts as held after {{ profile.tapping_term_ms }} ms
              <input type="range" min="80" max="500" step="10" :value="profile.tapping_term_ms" :data-test="'term-' + index" @change="updateProfile(index, { tapping_term_ms: Number(($event.target as HTMLInputElement).value) })" />
            </label>
            <label class="field">
              {{ profile.require_prior_idle_ms ? `Always a tap within ${profile.require_prior_idle_ms} ms of other typing` : 'No protection while typing fast' }}
              <input type="range" min="0" max="300" step="10" :value="profile.require_prior_idle_ms" @change="updateProfile(index, { require_prior_idle_ms: Number(($event.target as HTMLInputElement).value) })" />
            </label>
          </div>
        </div>
      </div>

      <!-- Combos --------------------------------------------------------------->
      <div v-else-if="section === 'combos'" key="combos" class="pane combos">
        <div class="col scroll">
          <div class="intro">
            <h2>Combos</h2>
            <p class="muted">Press keys together to get something else, for example J and K for Escape. A combo can also be keys pressed one after the other.</p>
          </div>
          <div class="row wrap">
            <button v-for="(c, index) in session.combos" :key="index" class="btn small" :class="{ primary: comboIndex === index }" @click="comboIndex = index">
              {{ c.keys.length ? comboName(c) : 'new combo' }}
            </button>
            <button class="btn small" :disabled="session.combos.length >= LIMITS.combos" data-test="add-combo" @click="addCombo">+ combo</button>
          </div>
          <template v-if="combo">
            <div v-if="tunable" class="row wrap">
              <span class="muted small">Pressed</span>
              <button class="btn small" :class="{ primary: !combo.sequence }" data-test="combo-together" @click="setSequence(false)">together</button>
              <button class="btn small" :class="{ primary: combo.sequence }" data-test="combo-sequence" @click="setSequence(true)">one after the other</button>
            </div>
            <p v-else class="note">Update the firmware to make a combo of keys pressed one after the other, of more than four keys, or for chosen layers only.</p>
            <p class="muted small">
              {{ combo.sequence ? 'Click the keys in the order they are pressed' : 'Click the keys that trigger it' }}
              ({{ combo.keys.length }} of up to {{ keyLimit }}).
            </p>
            <KeyboardView :layout="session.layout" :legend="baseLegend" :selected="combo.keys" :pressed="session.pressed" :knobs="knobPositions(session)" interactive data-test="combo-board" @down="toggleComboKey" />
            <p v-if="combo.keys.length < 2" class="note warn">Pick at least two keys, otherwise the combo replaces that key.</p>
            <p v-if="combo.sequence && combo.keys.length" class="small" data-test="combo-order">Order: {{ comboName(combo) }}</p>
            <p v-if="combo.sequence" class="note">
              The first key waits for the rest: pressed on its own, it types only once the time below has passed or another key follows.
              Start a sequence with a key you do not type with.
            </p>
            <label class="field">{{ combo.sequence ? 'Each key must follow the one before within' : 'All keys must go down within' }} {{ combo.timeout_ms }} ms
              <input type="range" :min="timing.min" :max="timing.max" :step="timing.step" :value="combo.timeout_ms" data-test="combo-time" @change="patchCombo({ timeout_ms: Number(($event.target as HTMLInputElement).value) })" />
            </label>
            <div v-if="tunable && session.layers.length > 1" class="row wrap">
              <span class="muted small">Works on</span>
              <button class="btn small" :class="{ primary: !combo.layers }" data-test="combo-all-layers" @click="patchCombo({ layers: 0 })">every layer</button>
              <button v-for="(_, layer) in session.layers" :key="layer" class="btn small" :class="{ primary: onLayer(layer) }" :data-test="'combo-layer-' + layer" @click="toggleComboLayer(layer)">
                Layer {{ layer }}
              </button>
            </div>
            <div class="row">
              <span>Result: <strong data-test="combo-result">{{ describeAction(combo.action) }}</strong></span>
              <span class="grow" />
              <button class="btn small danger" data-test="remove-combo" @click="removeCombo">Remove combo</button>
            </div>
          </template>
        </div>
        <aside v-if="combo" class="card">
          <ActionPicker :action="combo.action" :layer-count="session.layers.length" :current-layer="-1" :session="session" :layer-colors="session.layerColors" @change="(action: Action) => patchCombo({ action })" />
        </aside>
      </div>

      <!-- Macros --------------------------------------------------------------->
      <div v-else key="macros" class="pane macros">
        <div class="col scroll macro-list">
          <div class="intro">
            <h2>Macros</h2>
            <p class="muted">One key that presses a whole series of keys for you. Make one here, then put it on a key in the keymap.</p>
          </div>
          <button v-for="(m, index) in session.macros" :key="index" class="macro-item" :class="{ on: macroIndex === index, faint: !m.length }" :data-test="'macro-slot-' + index" @click="macroIndex = index">
            <span class="num">M{{ index + 1 }}</span>
            <span class="grow nm">{{ session.macroNames[index] || (m.length ? 'unnamed' : 'empty') }}</span>
            <span v-if="m.length" class="muted small">{{ m.length }}</span>
          </button>
        </div>
        <div class="card scroll">
          <MacroEditor :session="session" :index="macroIndex" guide />
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.behaviours {
  display: grid;
  grid-template-columns: 170px minmax(0, 1fr);
  gap: 18px;
  height: 100%;
  min-height: 0;
  padding: 18px 24px;
}

.sections {
  gap: 4px;
}

.section {
  text-align: left;
  padding: 9px 13px;
  border-radius: 10px;
  border: none;
  background: none;
  color: var(--muted);
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}

.section:hover {
  color: var(--text);
}

.section.on {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}

.pane {
  min-height: 0;
  gap: 16px;
}

.intro {
  max-width: 640px;
}

.small {
  font-size: 0.84rem;
}

.profiles {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(290px, 1fr));
  gap: 14px;
}

.flavors {
  gap: 6px;
}

.flavor {
  display: flex;
  gap: 10px;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 10px;
  cursor: pointer;
  transition: border-color 0.15s;
}

.flavor.on {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.flavor input,
input[type='range'] {
  accent-color: var(--accent);
}

.combos {
  display: grid;
  grid-template-rows: minmax(0, 1fr) minmax(200px, 42%);
  gap: 14px;
}

.combos :deep(svg.keyboard) {
  max-width: min(100%, 520px) !important;
  flex: none;
}

.combos > .col {
  min-height: 0;
  gap: 14px;
}

.combos aside {
  min-height: 0;
  padding: 14px 16px;
}

.wrap {
  flex-wrap: wrap;
}

.faint:not(.primary) {
  opacity: 0.55;
}

.macros {
  display: grid;
  grid-template-columns: 250px minmax(0, 1fr);
  gap: 18px;
}

.macros > * {
  min-height: 0;
}

.macro-list {
  gap: 3px;
}

.macro-list .intro {
  margin-bottom: 8px;
}

.macro-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 11px;
  border-radius: 9px;
  border: 1px solid transparent;
  background: none;
  text-align: left;
  cursor: pointer;
  transition:
    background 0.15s,
    border-color 0.15s;
}

.macro-item:hover {
  background: var(--panel);
}

.macro-item.on {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.macro-item .num {
  width: 30px;
  color: var(--muted);
  font-size: 0.8rem;
  font-variant-numeric: tabular-nums;
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
</style>
