<script setup lang="ts">
// The timing-based parts of a keymap: hold-tap feel, combos and macros.
import { computed, ref } from 'vue'
import ActionPicker from '../components/ActionPicker.vue'
import KeyboardView from '../components/KeyboardView.vue'
import { attempt } from '../lib/app'
import { describeAction, describeStep, KEY_GROUPS, legend, textToSteps } from '../lib/keycodes'
import type { Action, Combo, HoldTapFlavor, HoldTapProfile, MacroStep } from '../lib/protocol'
import { LIMITS } from '../lib/protocol'
import { knobPositions, setCombos, setMacro, setProfile, type Session } from '../lib/session'

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
  const combos = props.session.combos.map((c, i) => (i === comboIndex.value ? { ...c, ...change } : c))
  return saveCombos(combos)
}

function toggleComboKey(pos: number) {
  if (!combo.value) return
  const keys = combo.value.keys.includes(pos) ? combo.value.keys.filter((k) => k !== pos) : [...combo.value.keys, pos]
  if (keys.length <= LIMITS.comboKeys) patchCombo({ keys })
}

async function removeCombo() {
  await saveCombos(props.session.combos.filter((_, i) => i !== comboIndex.value))
  comboIndex.value = Math.max(0, comboIndex.value - 1)
}

const baseLegend = (pos: number) => (props.session.layers[0]?.[pos] ? legend(props.session.layers[0][pos]) : undefined)

// --- Macros ------------------------------------------------------------------
const macroIndex = ref(0)
const steps = computed(() => props.session.macros[macroIndex.value] ?? [])
const text = ref('')
const stepsUsed = computed(() => props.session.macros.reduce((sum, m) => sum + m.length, 0))
const newKey = ref(0x28)
const newDelay = ref(100)
const saveSteps = (next: MacroStep[]) => attempt(() => setMacro(props.session, macroIndex.value, next))

async function typeText() {
  await saveSteps([...steps.value, ...textToSteps(text.value)])
  text.value = ''
}
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
            <p class="muted">Press two to four keys together to get something else, for example J and K for Escape.</p>
          </div>
          <div class="row wrap">
            <button v-for="(c, index) in session.combos" :key="index" class="btn small" :class="{ primary: comboIndex === index }" @click="comboIndex = index">
              {{ c.keys.length ? c.keys.map((k) => baseLegend(k)?.main || k).join(' + ') : 'new combo' }}
            </button>
            <button class="btn small" :disabled="session.combos.length >= LIMITS.combos" data-test="add-combo" @click="addCombo">+ combo</button>
          </div>
          <template v-if="combo">
            <p class="muted small">Click the keys that trigger it ({{ combo.keys.length }} of up to {{ LIMITS.comboKeys }}).</p>
            <KeyboardView :layout="session.layout" :legend="baseLegend" :selected="combo.keys" :pressed="session.pressed" :knobs="knobPositions(session)" interactive data-test="combo-board" @down="toggleComboKey" />
            <p v-if="combo.keys.length < 2" class="note warn">Pick at least two keys, otherwise the combo replaces that key.</p>
            <label class="field">All keys must go down within {{ combo.timeout_ms }} ms
              <input type="range" min="20" max="150" step="5" :value="combo.timeout_ms" @change="patchCombo({ timeout_ms: Number(($event.target as HTMLInputElement).value) })" />
            </label>
            <div class="row">
              <span>Result: <strong data-test="combo-result">{{ describeAction(combo.action) }}</strong></span>
              <span class="grow" />
              <button class="btn small danger" data-test="remove-combo" @click="removeCombo">Remove combo</button>
            </div>
          </template>
        </div>
        <aside v-if="combo" class="card">
          <ActionPicker :action="combo.action" :layer-count="session.layers.length" :current-layer="-1" :macros="session.macros.map((m) => m.length > 0)" :layer-colors="session.layerColors" @change="(action: Action) => patchCombo({ action })" />
        </aside>
      </div>

      <!-- Macros --------------------------------------------------------------->
      <div v-else key="macros" class="pane scroll col">
        <div class="intro">
          <h2>Macros</h2>
          <p class="muted">
            A macro presses a series of keys for you. Put it on a key in the keymap under "macros".
            <span class="mono">{{ stepsUsed }} / {{ LIMITS.macroSteps }}</span> steps used.
          </p>
        </div>
        <div class="row wrap">
          <button v-for="(m, index) in session.macros" :key="index" class="btn small" :class="{ primary: macroIndex === index, faint: !m.length }" :data-test="'macro-slot-' + index" @click="macroIndex = index">
            M{{ index + 1 }}
          </button>
        </div>
        <div class="card col">
          <h3>Macro {{ macroIndex + 1 }}</h3>
          <div class="row">
            <input v-model="text" class="input grow" placeholder="Type text to add, e.g. an email address" data-test="macro-text" @keydown.enter="typeText" />
            <button class="btn" :disabled="!text" data-test="macro-add-text" @click="typeText">Add text</button>
          </div>
          <p class="muted small">Text is typed as on a US keyboard layout. Other layouts may produce different symbols.</p>
          <div class="row wrap">
            <select v-model="newKey" class="input">
              <optgroup v-for="group in KEY_GROUPS" :key="group.title" :label="group.title">
                <option v-for="k in group.keys" :key="k.usage" :value="k.usage">{{ k.name }}</option>
              </optgroup>
            </select>
            <button class="btn small" @click="saveSteps([...steps, { Tap: newKey }])">+ tap</button>
            <button class="btn small" @click="saveSteps([...steps, { Press: newKey }])">+ hold</button>
            <button class="btn small" @click="saveSteps([...steps, { Release: newKey }])">+ release</button>
            <input v-model.number="newDelay" class="input delay" type="number" min="1" max="10000" />
            <button class="btn small" @click="saveSteps([...steps, { DelayMs: Math.min(60000, Math.max(1, newDelay)) }])">+ wait (ms)</button>
          </div>
          <TransitionGroup name="pop" tag="ol" class="steps" data-test="macro-steps">
            <li v-for="(step, index) in steps" :key="index + JSON.stringify(step)">
              <span>{{ describeStep(step) }}</span>
              <button class="btn small ghost" title="Remove step" @click="saveSteps(steps.filter((_, i) => i !== index))">×</button>
            </li>
          </TransitionGroup>
          <p v-if="!steps.length" class="muted">This macro is empty.</p>
          <button v-else class="btn small danger" data-test="macro-clear" @click="saveSteps([])">Clear macro</button>
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

.delay {
  width: 90px;
}

.steps {
  margin: 0;
  padding-left: 26px;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  gap: 2px 28px;
}

.steps li {
  padding-right: 4px;
}

.steps li > * {
  vertical-align: middle;
}
</style>
