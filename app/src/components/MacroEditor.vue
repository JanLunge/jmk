<script setup lang="ts">
// One macro, top to bottom in the order one makes it: what it is called, how
// it is recorded, and the steps that came out. Used under Behaviours and
// inside the key picker of the keymap.
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { attempt } from '../lib/app'
import { describeStep, isModifierUsage, KEY_GROUPS, keyLabel, keyName, textToSteps, usageForCode } from '../lib/keycodes'
import { language } from '../lib/language'
import type { MacroStep } from '../lib/protocol'
import { LIMITS } from '../lib/protocol'
import { recordingKeys, recordingToSteps, type Stroke, type Timing } from '../lib/recording'
import { canNameMacros, setMacro, setMacroName, type Session } from '../lib/session'

const props = defineProps<{
  session: Session
  index: number
  /** With room to spare: explains each part in a sentence. */
  guide?: boolean
}>()

const steps = computed(() => props.session.macros[props.index] ?? [])
const stepsUsed = computed(() => props.session.macros.reduce((sum, m) => sum + m.length, 0))
const saveSteps = (next: MacroStep[]) => attempt(() => setMacro(props.session, props.index, next))
/** A change made by hand: the recording is then no longer what the macro is. */
function edit(next: MacroStep[]) {
  take.value = undefined
  return saveSteps(next)
}

// --- Name ----------------------------------------------------------------------
// Typed into a draft, so that the keyboard reporting meanwhile does not wipe it.
const nameDraft = ref('')
const naming = ref(false)
watch(
  () => [props.index, props.session.macroNames[props.index]] as const,
  ([, name]) => !naming.value && (nameDraft.value = name ?? ''),
  { immediate: true },
)
async function rename() {
  naming.value = false
  const name = nameDraft.value.trim()
  if (name === (props.session.macroNames[props.index] ?? '')) return
  await attempt(() => setMacroName(props.session, props.index, name))
  nameDraft.value = props.session.macroNames[props.index] ?? ''
}

// --- Recording -----------------------------------------------------------------
// The keys pressed on this computer's keyboard, stored as steps. The strokes
// are kept, so the timing can still be changed afterwards.
const PAUSES = [
  { ms: 0, name: 'no pause between keys' },
  { ms: 30, name: 'short pause between keys (30 ms)' },
  { ms: 100, name: 'clear pause between keys (100 ms)' },
  { ms: 300, name: 'long pause between keys (300 ms)' },
]
const TIMINGS: { id: Timing; name: string; text: string }[] = [
  { id: 'order', name: 'Just the keys', text: 'The same keys in the same order, as quickly as the computer takes them. Right for shortcuts: select all, then copy.' },
  { id: 'played', name: 'As I played it', text: 'Keeps every pause and how long each key was held. For when something needs time, like a window that has to open first.' },
]
const recording = ref(false)
const strokes = ref<Stroke[]>([])
const take = ref<{ before: MacroStep[]; strokes: Stroke[] }>()
const timing = ref<Timing>('order')
const pause = ref(0)
const heard = computed(() => strokes.value.filter((s) => s.down).map((s) => keyLabel(s.usage)))
const down = new Set<number>()

function onKey(event: KeyboardEvent) {
  const usage = usageForCode(event.code)
  event.preventDefault()
  event.stopPropagation()
  if (usage === undefined || event.repeat) return
  const isDown = event.type === 'keydown'
  const at = performance.now()
  // macOS does not report the release of a key pressed with Cmd, so letting go of Cmd lets go of those too.
  if (!isDown && (usage === 0xe3 || usage === 0xe7)) {
    for (const key of [...down].reverse()) {
      if (isModifierUsage(key)) continue
      down.delete(key)
      strokes.value.push({ usage: key, down: false, at })
    }
  }
  if (isDown) down.add(usage)
  else down.delete(usage)
  strokes.value.push({ usage, down: isDown, at })
}

function listen(on: boolean) {
  for (const type of ['keydown', 'keyup'] as const) {
    if (on) window.addEventListener(type, onKey, true)
    else window.removeEventListener(type, onKey, true)
  }
  if (on) window.addEventListener('blur', stopRecording)
  else window.removeEventListener('blur', stopRecording)
}

function record() {
  take.value = undefined
  strokes.value = []
  down.clear()
  recording.value = true
  recordingKeys.value = true
  listen(true)
}

function stopRecording() {
  if (!recording.value) return
  listen(false)
  recording.value = false
  recordingKeys.value = false
  if (!strokes.value.some((s) => s.down)) return
  take.value = { before: steps.value, strokes: strokes.value }
  return applyTake()
}

const applyTake = () => take.value && saveSteps([...take.value.before, ...recordingToSteps(take.value.strokes, timing.value, pause.value)])
watch([timing, pause], applyTake)
watch(
  () => props.index,
  () => {
    stopRecording()
    take.value = undefined
    adding.value = false
  },
)
onBeforeUnmount(() => {
  listen(false)
  if (recording.value) recordingKeys.value = false
})

// --- Steps by hand -------------------------------------------------------------
const adding = ref(false)
const text = ref('')
const newKey = ref(0x28)
const newDelay = ref(100)
async function typeText() {
  await edit([...steps.value, ...textToSteps(text.value)])
  text.value = ''
}
const clampMs = (ms: number) => Math.min(60000, Math.max(1, Math.round(ms) || 1))
const setDelay = (at: number, ms: number) => edit(steps.value.map((step, n) => (n === at ? { DelayMs: clampMs(ms) } : step)))

/** How a step is drawn: a key cap for a tap, an arrow down or up for hold and release. */
function chip(step: MacroStep): { kind: 'tap' | 'press' | 'release' | 'wait'; label: string } {
  if ('Tap' in step) return { kind: 'tap', label: keyLabel(step.Tap) }
  if ('Press' in step) return { kind: 'press', label: keyLabel(step.Press) }
  if ('Release' in step) return { kind: 'release', label: keyLabel(step.Release) }
  return { kind: 'wait', label: String(step.DelayMs) }
}
</script>

<template>
  <div class="macro" :class="{ guide }" data-test="macro-editor">
    <div class="line">
      <span class="label">Name</span>
      <div class="row">
        <input
          v-model="nameDraft"
          class="input name"
          :placeholder="`Macro ${index + 1}`"
          :maxlength="LIMITS.macroNameLength"
          :disabled="!canNameMacros(session)"
          :title="canNameMacros(session) ? 'What this macro is for, e.g. Copy all' : 'Update the firmware under Hardware to name macros'"
          data-test="macro-name"
          @focus="naming = true"
          @blur="rename"
          @keydown.enter="($event.target as HTMLInputElement).blur()"
        />
        <span class="muted small count">
          {{ steps.length }} {{ steps.length === 1 ? 'step' : 'steps' }}<template v-if="guide"> · {{ stepsUsed }} of {{ LIMITS.macroSteps }} used by all macros</template>
        </span>
      </div>
    </div>

    <div class="line">
      <span class="label">Record</span>
      <div class="col body">
        <div class="row wrap">
          <button v-if="!recording" class="btn primary" data-test="macro-record" @click="record"><span class="dot" /> Record</button>
          <button v-else class="btn live" data-test="macro-stop" @click="stopRecording"><span class="square" /> Stop</button>
          <div class="seg" role="radiogroup">
            <button v-for="t in TIMINGS" :key="t.id" class="seg-btn" :class="{ on: timing === t.id }" :title="t.text" :data-test="'timing-' + t.id" @click="timing = t.id">{{ t.name }}</button>
          </div>
          <select v-if="timing === 'order'" v-model.number="pause" class="input pause" data-test="timing-pause">
            <option v-for="p in PAUSES" :key="p.ms" :value="p.ms">{{ p.name }}</option>
          </select>
        </div>
        <p v-if="guide" class="muted small">
          {{ TIMINGS.find((t) => t.id === timing)!.text }}
          <template v-if="take"> Switching rewrites what you just recorded; no need to record again.</template>
        </p>
      </div>
    </div>

    <div class="line">
      <span class="label">Steps</span>
      <div class="col body">
        <div v-if="recording" class="row wrap heard" data-test="macro-heard">
          <span v-if="!heard.length" class="muted">Listening. Press the keys on this computer's keyboard, then click Stop.</span>
          <span v-for="(name, n) in heard" :key="n" class="chip accent">{{ name }}</span>
        </div>
        <template v-else>
          <TransitionGroup name="pop" tag="ol" class="steps" data-test="macro-steps">
            <li v-for="(step, n) in steps" :key="n + JSON.stringify(step)" class="step" :class="chip(step).kind" :title="describeStep(step)">
              <template v-if="chip(step).kind === 'wait'">
                <input class="ms" type="number" min="1" max="60000" :value="chip(step).label" title="Milliseconds to wait" @change="setDelay(n, Number(($event.target as HTMLInputElement).value))" /> ms
              </template>
              <template v-else>
                <span v-if="chip(step).kind !== 'tap'" class="arrow">{{ chip(step).kind === 'press' ? '↓' : '↑' }}</span>{{ chip(step).label }}
              </template>
              <button class="drop" title="Remove this step" @click="edit(steps.filter((_, i) => i !== n))">×</button>
            </li>
          </TransitionGroup>
          <p v-if="!steps.length" class="muted">Nothing yet. Click Record and press the keys, or add steps by hand.</p>
          <div class="row wrap tools">
            <button class="btn small ghost" :class="{ primary: adding }" data-test="macro-by-hand" @click="adding = !adding">+ Add by hand</button>
            <span v-if="steps.length" class="muted small legend">↓ holds a key down · ↑ lets it go</span>
            <span class="grow" />
            <button v-if="steps.length" class="btn small ghost danger" data-test="macro-clear" @click="edit([])">Clear all steps</button>
          </div>
          <div v-if="adding" class="by-hand col">
            <div class="row">
              <input v-model="text" class="input grow" placeholder="Text to type, e.g. an email address" data-test="macro-text" @keydown.enter="typeText" />
              <button class="btn small" :disabled="!text" data-test="macro-add-text" @click="typeText">Add text</button>
            </div>
            <div class="row wrap">
              <select v-model="newKey" class="input">
                <optgroup v-for="group in KEY_GROUPS" :key="group.title" :label="group.title">
                  <option v-for="k in group.keys" :key="k.usage" :value="k.usage">{{ keyName(k.usage) }}</option>
                </optgroup>
              </select>
              <button class="btn small" :title="'Press and release ' + keyName(newKey)" @click="edit([...steps, { Tap: newKey }])">+ tap</button>
              <button class="btn small" title="Press it and keep it down" @click="edit([...steps, { Press: newKey }])">+ ↓ hold</button>
              <button class="btn small" title="Let it go again" @click="edit([...steps, { Release: newKey }])">+ ↑ release</button>
              <input v-model.number="newDelay" class="input delay" type="number" min="1" max="60000" />
              <button class="btn small" @click="edit([...steps, { DelayMs: clampMs(newDelay) }])">+ wait (ms)</button>
            </div>
            <p class="muted small">Text is typed for a {{ language().platform === 'mac' ? 'Mac' : 'Windows or Linux computer' }} set to {{ language().name }}, as chosen in the key picker. Characters that need an accent key are left out.</p>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.macro {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}

.macro.guide {
  gap: 18px;
}

/* What each part is, down the left edge. */
.line {
  display: grid;
  grid-template-columns: 62px minmax(0, 1fr);
  gap: 12px;
  align-items: start;
}

.label {
  padding-top: 9px;
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--muted);
}

.body {
  gap: 8px;
  min-width: 0;
}

.body p {
  margin: 0;
  max-width: 620px;
}

.small {
  font-size: 0.82rem;
}

.wrap {
  flex-wrap: wrap;
}

.name {
  width: 220px;
  font-weight: 600;
}

.count {
  white-space: nowrap;
}

.dot,
.square {
  display: inline-block;
  width: 9px;
  height: 9px;
  margin-right: 4px;
  background: currentColor;
}

.dot {
  border-radius: 50%;
}

.square {
  border-radius: 2px;
  color: var(--danger);
  animation: blink 1s ease-in-out infinite;
}

.btn.live {
  border-color: var(--danger);
}

@keyframes blink {
  50% {
    opacity: 0.25;
  }
}

.seg {
  display: flex;
  padding: 2px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: var(--panel-2);
}

.seg-btn {
  padding: 5px 12px;
  border: none;
  border-radius: 8px;
  background: none;
  color: var(--muted);
  cursor: pointer;
  white-space: nowrap;
  transition:
    background 0.15s,
    color 0.15s;
}

.seg-btn.on {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}

.pause {
  padding-top: 6px;
  padding-bottom: 6px;
}

.heard {
  gap: 5px;
  min-height: 34px;
}

.steps {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.step {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 30px;
  padding: 0 4px 0 9px;
  border-radius: 8px;
  border: 1px solid var(--line);
  background: var(--panel-2);
  font-size: 0.86rem;
  white-space: nowrap;
}

.step.tap {
  background: var(--cap-top);
  border-color: transparent;
  border-bottom: 2px solid var(--cap);
  font-weight: 600;
}

.step.wait {
  border-style: dashed;
  background: none;
  color: var(--muted);
}

.arrow {
  color: var(--accent);
  font-weight: 700;
}

.ms {
  width: 52px;
  padding: 1px 2px;
  border: none;
  border-bottom: 1px solid var(--line);
  background: none;
  color: var(--text);
  font: inherit;
  text-align: right;
  outline: none;
}

.ms:focus {
  border-bottom-color: var(--accent);
}

.drop {
  padding: 0 4px;
  border: none;
  background: none;
  color: var(--muted);
  cursor: pointer;
  opacity: 0.45;
  transition: opacity 0.15s;
}

.step:hover .drop,
.drop:focus-visible {
  opacity: 1;
}

.tools {
  gap: 10px;
}

.by-hand {
  gap: 8px;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
}

.delay {
  width: 84px;
}
</style>
