<script setup lang="ts">
// First-time setup: from an unconfigured board to a working keyboard.
import { computed, onBeforeUnmount, reactive, ref, watch } from 'vue'
import BoardDiagram, { type PinRole } from '../components/BoardDiagram.vue'
import KeyboardView from '../components/KeyboardView.vue'
import MatrixExplainer from '../components/MatrixExplainer.vue'
import { app, attempt, partnerOf, toast } from '../lib/app'
import { boardsFor, findBoard, genericBoard, pinLabel, wirablePins } from '../lib/boards'
import { KeyCollector, KnobFinder } from '../lib/collector'
import { inferWiring, type KeySignature } from '../lib/infer'
import { legend } from '../lib/keycodes'
import { hasKey } from '../lib/layout'
import type { Contact, EncoderConfig, HardwareConfig, LinkConfig, Pin, SplitConfig } from '../lib/protocol'
import { LIMITS } from '../lib/protocol'
import { role, save, setAction, setHardware, setLayout, setMeta, starterAction, starterLayout, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

const STEPS = ['learn', 'board', 'shape', 'keys', 'result', 'finish'] as const
type Step = (typeof STEPS)[number]
const step = ref<Step>('learn')
const stepIndex = computed(() => STEPS.indexOf(step.value))
const direction = ref<'forward' | 'back'>('forward')

function go(to: Step) {
  direction.value = STEPS.indexOf(to) > stepIndex.value ? 'forward' : 'back'
  step.value = to
}

// --- Board -----------------------------------------------------------------
const info = props.session.info
const boards = computed(() => [...boardsFor(info.mcu), genericBoard(info.mcu, info.usable_pins)])
const boardId = ref(props.session.meta.controller || boards.value[0].id)
const board = computed(() => findBoard(boardId.value, info.mcu, info.usable_pins))
const reserved = reactive(new Set<Pin>(props.session.hardware.reserved_pins))

function toggleReserved(pin: Pin) {
  if (reserved.has(pin)) reserved.delete(pin)
  else reserved.add(pin)
}

// --- One piece or two --------------------------------------------------------
const partner = computed(() => partnerOf(props.session))
const partnerIsCentral = computed(() => !!partner.value && role(partner.value) === 'central')
const shape = ref<'single' | 'central' | 'peripheral'>(
  partnerIsCentral.value ? 'peripheral' : props.session.demo?.link ? 'central' : 'single',
)

/** Pin pairs that can carry the cable between halves on this chip. */
const linkPairs = computed<{ tx: Pin; rx: Pin }[]>(() => {
  const free = wirablePins(board.value, info.usable_pins).filter((pin) => !reserved.has(pin))
  if (info.mcu === 'Nrf52840') {
    // Any two pins can be a serial port on this chip.
    return free.slice(0, -1).map((tx, i) => ({ tx, rx: free[i + 1] }))
  }
  const serialPorts = [[0, 1], [12, 13], [16, 17], [28, 29], [4, 5], [8, 9], [20, 21], [24, 25]]
  return serialPorts.filter(([tx, rx]) => free.includes(tx) && free.includes(rx)).map(([tx, rx]) => ({ tx, rx }))
})
const link = ref(props.session.demo?.link ?? linkPairs.value[0])
const split = computed<SplitConfig>(() => {
  if (shape.value === 'single' || !link.value) return 'None'
  const config: LinkConfig = { Uart: { tx: link.value.tx, rx: link.value.rx } }
  return shape.value === 'central' ? { Central: config } : { Peripheral: config }
})
const linkPins = computed<Pin[]>(() => (split.value === 'None' || !link.value ? [] : [link.value.tx, link.value.rx]))

// --- Finding the keys --------------------------------------------------------
const collector = ref(reactive(new KeyCollector()))
const encoders = reactive<EncoderConfig[]>([])
const knobFinder = ref<KnobFinder>()
const bump = ref(0)
const candidates = computed(() =>
  wirablePins(board.value, info.usable_pins).filter((pin) => !reserved.has(pin) && !linkPins.value.includes(pin)),
)

function onProbe(contacts: Contact[]) {
  if (knobFinder.value) {
    const pins = knobFinder.value.update(contacts)
    if (pins) {
      encoders.push({ a: pins[0], b: pins[1], steps_per_detent: 4 })
      // Turning a knob can look like key presses on its pins; drop those.
      const keys = collector.value.keys
      for (let i = keys.length - 1; i >= 0; i--) {
        const key = keys[i]
        if (key.kind !== 'link' && pins.includes(key.pin)) collector.value.remove(i)
      }
      knobFinder.value = undefined
    }
    return
  }
  const encoderPins = new Set(encoders.flatMap((e) => [e.a, e.b]))
  const relevant = contacts.filter((c) => !('Ground' in c && encoderPins.has(c.Ground)))
  collector.value.update(relevant)
  bump.value++
}

async function startProbing() {
  collector.value = reactive(new KeyCollector())
  encoders.splice(0)
  props.session.client.onProbe = onProbe
  await attempt(() => props.session.client.startProbe(candidates.value))
}

async function stopProbing() {
  knobFinder.value = undefined
  props.session.client.onProbe = () => {}
  await props.session.client.stopProbe().catch(() => {})
}

function findKnob() {
  const used = new Set(collector.value.keys.flatMap((key) => (key.kind === 'link' ? [key.driven, key.sensed] : [key.pin])))
  knobFinder.value = new KnobFinder(used)
}

watch(step, async (now, before) => {
  if (now === 'keys' && before !== 'result') await startProbing()
  if (before === 'keys' && now !== 'result') await stopProbing()
})
onBeforeUnmount(stopProbing)

function describeKey(key: KeySignature): string {
  const name = (pin: Pin) => pinLabel(board.value, pin)
  if (key.kind === 'ground') return `${name(key.pin)} to ground`
  if (key.kind === 'supply') return `${name(key.pin)} to supply`
  return `${name(key.driven)} ↔ ${name(key.sensed)}${key.diode ? ' with diode' : ''}`
}

// --- Result ------------------------------------------------------------------
const result = computed(() => inferWiring(collector.value.keys))
const hardware = computed<HardwareConfig | undefined>(() =>
  result.value.ok
    ? {
        matrix: result.value.wiring.matrix,
        reserved_pins: [...reserved],
        debounce_ms: 5,
        encoders: [...encoders],
        split: split.value,
      }
    : undefined,
)

const roles = computed(() => {
  const map = new Map<Pin, PinRole>()
  if (step.value === 'keys') candidates.value.forEach((pin) => map.set(pin, 'candidate'))
  reserved.forEach((pin) => map.set(pin, 'reserved'))
  if (stepIndex.value >= STEPS.indexOf('shape')) linkPins.value.forEach((pin) => map.set(pin, 'link'))
  encoders.forEach((e) => [e.a, e.b].forEach((pin) => map.set(pin, 'encoder')))
  if (step.value === 'result' && result.value.ok) {
    const matrix = result.value.wiring.matrix
    if (matrix !== 'None' && 'Direct' in matrix) matrix.Direct.pins.forEach((pin) => map.set(pin, 'direct'))
    if (matrix !== 'None' && 'Matrix' in matrix) {
      matrix.Matrix.rows.forEach((pin) => map.set(pin, 'row'))
      matrix.Matrix.cols.forEach((pin) => map.set(pin, 'col'))
    }
  }
  return map
})

/** The found matrix in the explainer's terms: which lines are switched, which read. */
const scan = computed(() => {
  const matrix = result.value.ok ? result.value.wiring.matrix : 'None'
  if (matrix === 'None' || !('Matrix' in matrix)) return undefined
  const { rows, cols, diodes } = matrix.Matrix
  const names = (pins: Pin[]) => pins.map((pin) => pinLabel(board.value, pin))
  const keys = collector.value.keys.filter((k) => k.kind === 'link')
  return {
    driven: names(diodes === 'Col2Row' ? rows : cols),
    sensed: names(diodes === 'Col2Row' ? cols : rows),
    diodes: keys.every((k) => k.kind === 'link' && k.diode),
  }
})

const applying = ref(false)
async function apply() {
  if (!hardware.value || !result.value.ok) return
  applying.value = true
  const done = await attempt(async () => {
    const config = hardware.value!
    await stopProbing()
    await setHardware(props.session, config)
    const positions = result.value.ok ? result.value.wiring.positions : []
    await setLayout(props.session, starterLayout(config, positions))
    if (shape.value !== 'peripheral') {
      // Letters in the order the keys sit in the grid, so the tester shows life at once.
      const ordered = [...positions].sort((a, b) => a - b)
      for (const [index, pos] of ordered.entries()) await setAction(props.session, 0, pos, starterAction(index))
      const firstKnob = props.session.layout.length - 2 * config.encoders.length
      for (const index of config.encoders.keys()) {
        await setAction(props.session, 0, firstKnob + 2 * index, { Basic: { Consumer: 0xea } })
        await setAction(props.session, 0, firstKnob + 2 * index + 1, { Basic: { Consumer: 0xe9 } })
      }
    }
    await setMeta(props.session, { name: name.value.trim().slice(0, LIMITS.nameLength), controller: board.value.id })
    return true
  })
  applying.value = false
  if (done) go('finish')
}

// --- Finish ------------------------------------------------------------------
const name = ref(props.session.meta.name || props.session.demo?.label || '')
const keyLegend = (pos: number) => (props.session.layers[0]?.[pos] ? legend(props.session.layers[0][pos]) : undefined)
const keyCount = computed(() => props.session.layout.filter(hasKey).length)

async function finish(next: 'editor' | 'partner') {
  const done = await attempt(async () => {
    await setMeta(props.session, { name: name.value.trim().slice(0, LIMITS.nameLength), controller: board.value.id })
    await save(props.session)
    return true
  })
  if (!done) return
  toast('Saved to the keyboard')
  props.session.setup = false
  if (next === 'partner' && partner.value) app.currentId = partner.value.id
}

const canContinue = computed(() => {
  if (step.value === 'shape') return shape.value === 'single' || !!link.value
  if (step.value === 'keys') return collector.value.keys.length > 0 && !knobFinder.value
  return true
})

function next() {
  const order: Step[] = ['learn', 'board', 'shape', 'keys', 'result']
  go(order[order.indexOf(step.value) + 1])
}
</script>

<template>
  <div class="wizard">
    <header class="top row">
      <div class="dots row">
        <span v-for="(s, i) in STEPS" :key="s" class="dot" :class="{ done: i < stepIndex, now: i === stepIndex }" />
      </div>
      <span class="grow" />
      <span class="muted">{{ session.transport.label }} · {{ info.mcu }}</span>
    </header>

    <Transition :name="direction === 'forward' ? 'slide' : 'slide-back'" mode="out-in">
      <!-- 1 ------------------------------------------------------------------>
      <section v-if="step === 'learn'" key="learn" class="page two">
        <div class="col text">
          <span class="chip accent">How a keyboard works</span>
          <h1>A tiny computer asking one question, a thousand times a second.</h1>
          <p class="muted">
            <em>Which switches are closed right now?</em> Your controller has far fewer pins than a keyboard has keys,
            so keys are usually wired in a grid. The chip switches on one line at a time and watches the others.
          </p>
          <p class="muted">
            You do not need to know how yours is wired. In a moment you will press every key once, and the firmware
            will work it out.
          </p>
        </div>
        <MatrixExplainer />
      </section>

      <!-- 2 ------------------------------------------------------------------>
      <section v-else-if="step === 'board'" key="board" class="page two">
        <div class="col text">
          <span class="chip accent">Step 1 · Your controller</span>
          <h1>Which board is the brain?</h1>
          <div class="choices col">
            <label v-for="option in boards" :key="option.id" class="choice" :class="{ on: boardId === option.id }">
              <input v-model="boardId" type="radio" :value="option.id" />
              <div>
                <strong>{{ option.name }}</strong>
                <div v-if="option.alsoKnownAs" class="muted small">{{ option.alsoKnownAs }}</div>
              </div>
            </label>
          </div>
          <div class="note" :class="{ warn: reserved.size }">
            <strong>Anything besides keys soldered on?</strong>
            A display, LEDs, a battery circuit: click those pins on the picture. JMK will never send anything to a
            pin marked red.
            <span v-if="reserved.size" class="mono"> {{ reserved.size }} protected.</span>
          </div>
          <p v-if="board.unverified" class="muted small">
            This board's picture has not been checked against a real one yet. Compare the pin names with yours.
          </p>
        </div>
        <BoardDiagram :board="board" :roles="roles" selectable data-test="board" @toggle="toggleReserved" />
      </section>

      <!-- 3 ------------------------------------------------------------------>
      <section v-else-if="step === 'shape'" key="shape" class="page two">
        <div class="col text">
          <span class="chip accent">Step 2 · Shape</span>
          <h1>One piece, or two halves?</h1>
          <div class="choices col">
            <label class="choice" :class="{ on: shape === 'single' }">
              <input v-model="shape" type="radio" value="single" data-test="shape-single" />
              <div><strong>One piece</strong>
                <div class="muted small">A macro pad or a normal keyboard with a single controller.</div></div>
            </label>
            <label class="choice" :class="{ on: shape === 'central' }">
              <input v-model="shape" type="radio" value="central" data-test="shape-central" />
              <div><strong>The half that plugs into the computer</strong>
                <div class="muted small">It keeps the keymap and listens to the other half over the cable.</div></div>
            </label>
            <label class="choice" :class="{ on: shape === 'peripheral' }">
              <input v-model="shape" type="radio" value="peripheral" data-test="shape-peripheral" />
              <div><strong>The other half</strong>
                <div class="muted small">It only reports its keys. Set this one up over its own USB port too.</div></div>
            </label>
          </div>
          <Transition name="rise">
            <div v-if="shape !== 'single'" class="col">
              <label class="field">Cable between the halves
                <select v-model="link" class="input" data-test="link">
                  <option v-for="pair in linkPairs" :key="pair.tx" :value="pair">
                    sends on {{ pinLabel(board, pair.tx) }}, receives on {{ pinLabel(board, pair.rx) }}
                  </option>
                </select>
              </label>
              <p class="muted small">
                Each half's sending pin must be wired to the other half's receiving pin, plus ground and power.
                <template v-if="!linkPairs.length">No free pin pair on this board can carry a serial cable.</template>
              </p>
            </div>
          </Transition>
        </div>
        <BoardDiagram :board="board" :roles="roles" />
      </section>

      <!-- 4 ------------------------------------------------------------------>
      <section v-else-if="step === 'keys'" key="keys" class="page two">
        <div class="col text">
          <span class="chip accent">Step 3 · Find the keys</span>
          <h1>Press every key once.</h1>
          <p class="muted">
            Any order. Watch the board: the pins your key connects light up. Only weak test currents are used, and
            only on the yellow pins.
          </p>
          <div class="found" data-test="found">
            <TransitionGroup name="pop">
              <div
                v-for="(key, index) in collector.keys"
                :key="describeKey(key)"
                class="found-key"
                :class="{ last: collector.last === index }"
                :title="describeKey(key)"
              >
                <span :key="collector.last === index ? bump : 0" class="cap">{{ index + 1 }}</span>
                <button class="remove" title="Not a key" @click="collector.remove(index)">×</button>
              </div>
            </TransitionGroup>
            <span v-if="!collector.keys.length" class="muted waiting">Waiting for the first key…</span>
          </div>
          <div class="row">
            <strong data-test="count">{{ collector.keys.length }} {{ collector.keys.length === 1 ? 'key' : 'keys' }}</strong>
            <span v-if="collector.last !== undefined" class="muted mono">{{ describeKey(collector.keys[collector.last]) }}</span>
          </div>
          <Transition name="rise">
            <div v-if="collector.confused" class="note warn">
              That looked like more than one key at the same instant. Press them one after the other.
            </div>
          </Transition>
          <div v-if="collector.stuck.length" class="note warn">
            Some pins were already connected before you pressed anything, so they are ignored. If that is not a
            stuck key, mark them as protected in step 1.
          </div>
          <div class="knobs col">
            <div v-for="(encoder, index) in encoders" :key="encoder.a" class="row">
              <span class="chip">knob {{ index + 1 }}</span>
              <span class="mono">{{ pinLabel(board, encoder.a) }} + {{ pinLabel(board, encoder.b) }}</span>
              <button class="btn small ghost" @click="encoders.splice(index, 1)">remove</button>
            </div>
            <div v-if="knobFinder" class="note">
              <strong>Turn the knob a few clicks back and forth…</strong>
              <button class="btn small ghost" @click="knobFinder = undefined">cancel</button>
            </div>
            <button v-else-if="encoders.length < LIMITS.encoders" class="btn small" data-test="find-knob" @click="findKnob">
              + This board has a rotary knob
            </button>
          </div>
        </div>
        <BoardDiagram :board="board" :roles="roles" :active="collector.active" />
      </section>

      <!-- 5 ------------------------------------------------------------------>
      <section v-else-if="step === 'result'" key="result" class="page two">
        <div class="col text">
          <span class="chip accent">Step 4 · What we found</span>
          <template v-if="result.ok">
            <h1>{{ collector.keys.length }} keys, wiring understood.</h1>
            <ul class="notes">
              <li v-for="note in result.wiring.notes" :key="note">{{ note }}</li>
              <li v-if="encoders.length">{{ encoders.length }} rotary knob{{ encoders.length > 1 ? 's' : '' }}.</li>
              <li v-if="shape !== 'single'">Cable to the other half on the pink pins.</li>
            </ul>
            <div class="legend row">
              <span class="chip"><i class="sw row-c" />rows</span>
              <span class="chip"><i class="sw col-c" />columns</span>
              <span class="chip"><i class="sw direct-c" />one key each</span>
              <span class="chip"><i class="sw reserved-c" />protected</span>
            </div>
            <label class="field">Name your keyboard
              <input v-model="name" class="input" :maxlength="LIMITS.nameLength" placeholder="My keyboard" data-test="name" />
            </label>
          </template>
          <template v-else>
            <h1>Something is unusual.</h1>
            <div class="note warn">{{ result.problem }}</div>
            <p class="muted">Go back to press the keys again, or fix the wiring and retry.</p>
          </template>
        </div>
        <div class="col visuals">
          <MatrixExplainer v-if="scan" :driven="scan.driven" :sensed="scan.sensed" :diodes="scan.diodes" :held="[]" />
          <BoardDiagram v-else :board="board" :roles="roles" />
        </div>
      </section>

      <!-- 6 ------------------------------------------------------------------>
      <section v-else key="finish" class="page finish col">
        <span class="chip signal">It works</span>
        <h1 v-if="shape === 'peripheral'">This half is ready.</h1>
        <h1 v-else>Press a key. It already types.</h1>
        <p class="muted center">
          <template v-if="shape === 'peripheral'">
            It reports its {{ keyCount }} keys over the cable. The half that plugs into the computer decides what
            they do.
          </template>
          <template v-else>
            Every key got a letter for now. Next you can arrange the keys the way they sit on your desk and choose
            what each one does.
          </template>
        </p>
        <div class="tester">
          <KeyboardView :layout="session.layout" :legend="keyLegend" :pressed="session.pressed" data-test="tester" />
        </div>
        <div class="row">
          <button class="btn primary" data-test="finish" @click="finish('editor')">Save and continue</button>
          <button v-if="partner?.setup" class="btn" data-test="finish-partner" @click="finish('partner')">
            Save, then set up the {{ partner.demo?.label.toLowerCase() ?? 'other half' }}
          </button>
        </div>
      </section>
    </Transition>

    <footer v-if="step !== 'finish'" class="bottom row">
      <button class="btn ghost" :disabled="stepIndex === 0" data-test="back" @click="go(STEPS[stepIndex - 1])">Back</button>
      <span class="grow" />
      <button v-if="step === 'result'" class="btn primary" :disabled="!result.ok || applying" data-test="apply" @click="apply">
        {{ applying ? 'Setting up…' : 'Set up my keyboard' }}
      </button>
      <button v-else class="btn primary" :disabled="!canContinue" data-test="next" @click="next">
        {{ step === 'keys' ? "That's all of them" : 'Continue' }}
      </button>
    </footer>
  </div>
</template>

<style scoped>
.wizard {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.top,
.bottom {
  padding: 14px 28px;
  flex: none;
}

.bottom {
  border-top: 1px solid var(--line);
}

.dots {
  gap: 7px;
}

.dot {
  width: 26px;
  height: 5px;
  border-radius: 3px;
  background: var(--line);
  transition:
    background 0.3s,
    width 0.3s var(--ease);
}

.dot.done {
  background: var(--muted);
}

.dot.now {
  width: 44px;
  background: var(--accent);
}

.page {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 18px clamp(24px, 5vw, 72px) 28px;
}

.two {
  display: grid;
  grid-template-columns: minmax(300px, 1fr) minmax(280px, 1fr);
  gap: clamp(24px, 5vw, 64px);
  align-items: center;
}

.text {
  gap: 16px;
  align-items: flex-start;
  max-width: 520px;
}

.small {
  font-size: 0.85rem;
}

.choices {
  align-self: stretch;
  gap: 8px;
}

.choice {
  display: flex;
  gap: 12px;
  align-items: center;
  padding: 11px 14px;
  border: 1px solid var(--line);
  border-radius: 12px;
  cursor: pointer;
  transition:
    border-color 0.15s,
    background 0.15s,
    transform 0.15s var(--ease);
}

.choice:hover {
  transform: translateX(3px);
}

.choice.on {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.choice input {
  accent-color: var(--accent);
}

.found {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  min-height: 52px;
  align-self: stretch;
  align-items: center;
}

.found-key {
  position: relative;
}

.cap {
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  border-radius: 10px;
  background: var(--cap-top);
  border-bottom: 4px solid var(--cap);
  font-weight: 700;
}

.last .cap {
  background: var(--signal);
  color: #06201d;
  animation: bounce 0.35s var(--ease);
}

.remove {
  position: absolute;
  top: -6px;
  right: -6px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: none;
  background: var(--danger);
  color: white;
  font-size: 12px;
  line-height: 1;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s;
}

.found-key:hover .remove {
  opacity: 1;
}

.waiting {
  animation: breathe 1.6s ease-in-out infinite;
}

.knobs {
  align-self: stretch;
  gap: 8px;
}

.notes {
  margin: 0;
  padding-left: 20px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.legend {
  flex-wrap: wrap;
}

.sw {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  display: inline-block;
}
.row-c {
  background: var(--accent);
}
.col-c {
  background: #6ea8ff;
}
.direct-c {
  background: var(--ok);
}
.reserved-c {
  background: var(--danger);
}

.visuals {
  align-items: center;
}

.finish {
  align-items: center;
  justify-content: center;
  gap: 18px;
  text-align: center;
}

.center {
  max-width: 520px;
}

.tester {
  width: 100%;
  display: grid;
  place-items: center;
  padding: 18px;
}

.slide-enter-active,
.slide-back-enter-active {
  transition:
    opacity 0.3s var(--ease),
    transform 0.3s var(--ease);
}
.slide-leave-active,
.slide-back-leave-active {
  transition:
    opacity 0.14s,
    transform 0.14s;
}
.slide-enter-from {
  opacity: 0;
  transform: translateX(36px);
}
.slide-leave-to {
  opacity: 0;
  transform: translateX(-24px);
}
.slide-back-enter-from {
  opacity: 0;
  transform: translateX(-36px);
}
.slide-back-leave-to {
  opacity: 0;
  transform: translateX(24px);
}

@keyframes bounce {
  0% {
    transform: scale(0.8) translateY(3px);
  }
  60% {
    transform: scale(1.12);
  }
}

@keyframes breathe {
  50% {
    opacity: 0.4;
  }
}

@media (max-width: 860px) {
  .two {
    grid-template-columns: 1fr;
  }
}
</style>
