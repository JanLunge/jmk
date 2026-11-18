<script lang="ts">
// Where each keyboard's setup had got to, so it carries on there when the
// keyboard drops off for a firmware update and comes back.
const resume = new Map<string, { step: 'board' | 'shape' | 'keys'; shape: 'single' | 'central' | 'peripheral'; boardId: string }>()
</script>

<script setup lang="ts">
// First-time setup: from an unconfigured board to a working keyboard.
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import BoardDiagram, { type PinRole } from '../components/BoardDiagram.vue'
import KeyboardView from '../components/KeyboardView.vue'
import MatrixExplainer from '../components/MatrixExplainer.vue'
import { app, attempt, pairUp, partnerOf, toast } from '../lib/app'
import { boardsFor, findBoard, genericBoard, pinLabel, wirablePins } from '../lib/boards'
import { desktop, type BootDrive } from '../lib/desktop'
import { bootDrives, canFindCable, installOnBoard, updateFirmware } from '../lib/firmware'
import { chooseLink, findWires } from '../lib/cable'
import { KeyCollector, KnobFinder } from '../lib/collector'
import { inferWiring, type KeySignature } from '../lib/infer'
import { legend } from '../lib/keycodes'
import { hasKey } from '../lib/layout'
import type { Contact, EncoderConfig, HardwareConfig, LinkConfig, Pin, SplitConfig } from '../lib/protocol'
import { LIMITS } from '../lib/protocol'
import { isConfigured, knobPositions, needsRestart, restart, role, save, setAction, setHardware, setLayout, setMeta, starterAction, starterLayout, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

// Pressing the keys is the setup. The controller and the split pages are
// side trips for boards that need them; 'result' only shows when the
// wiring could not be read.
const STEPS = ['board', 'shape', 'keys', 'result', 'finish'] as const
type Step = (typeof STEPS)[number]
const resumed = resume.get(props.session.id)
const step = ref<Step>('keys')
const stepIndex = computed(() => STEPS.indexOf(step.value))
const direction = ref<'forward' | 'back'>('forward')

/** A side trip started from the key page ends there again. */
let fromKeys = false
function go(to: Step) {
  if (to === 'board' || to === 'shape') fromKeys = step.value === 'keys'
  direction.value = STEPS.indexOf(to) > stepIndex.value ? 'forward' : 'back'
  step.value = to
}

// --- Board -----------------------------------------------------------------
const info = props.session.info
const boards = computed(() => [...boardsFor(info.mcu), genericBoard(info.mcu, info.usable_pins)])
const boardId = ref(resumed?.boardId || props.session.meta.controller || boards.value[0].id)
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
  resumed
    ? resumed.shape
    : partnerIsCentral.value
    ? 'peripheral'
    : props.session.demo?.link || props.session.cable || props.session.hardware.split !== 'None'
      ? 'central'
      : 'single',
)

// A split has to know its cable before any key can be found.
step.value = resumed?.step ?? (shape.value === 'single' ? 'keys' : 'shape')

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
const link = ref(props.session.cable ?? props.session.demo?.link ?? linkPairs.value[0])
/** The pairs offered for choosing by hand, with the one that was found. */
const linkChoices = computed(() => {
  const found = props.session.cable
  return found && !linkPairs.value.some((pair) => pair.tx === found.tx && pair.rx === found.rx) ? [found, ...linkPairs.value] : linkPairs.value
})

watch(
  [step, shape, boardId],
  () => {
    const at = step.value === 'board' || step.value === 'shape' ? step.value : 'keys'
    if (step.value !== 'finish') resume.set(props.session.id, { step: at, shape: shape.value, boardId: boardId.value })
  },
  { immediate: true },
)

// --- Finding the cable by itself -----------------------------------------------
/** Other open keyboards that could be the second half. */
const others = computed(() =>
  app.sessions.filter((s) => s !== props.session && !s.closed && s.transport.kind === props.session.transport.kind),
)
const otherId = ref((partner.value ?? others.value[0])?.id)
const other = computed(() => others.value.find((s) => s.id === otherId.value) ?? others.value[0])
const cable = reactive({ state: 'idle' as 'idle' | 'searching' | 'found' | 'failed', progress: 0, problem: '', byHand: false })
if (props.session.cable) cable.state = 'found'

/** Pins of a half that a cable wire could be on. */
function cablePins(session: Session, protectedPins: Iterable<Pin>): Pin[] {
  const board = findBoard(session.meta.controller || (session === props.session ? boardId.value : ''), session.info.mcu, session.info.usable_pins)
  const taken = new Set<Pin>(protectedPins)
  if (session.lighting.pin !== null) taken.add(session.lighting.pin)
  return wirablePins(board, session.info.usable_pins).filter((pin) => !taken.has(pin))
}

async function findCable() {
  const second = other.value
  if (!second || cable.state === 'searching') return
  cable.state = 'searching'
  cable.progress = 0
  cable.problem = ''
  try {
    const wires = await findWires(
      props.session,
      second,
      cablePins(props.session, reserved),
      cablePins(second, second.hardware.reserved_pins),
      undefined,
      (done, total) => (cable.progress = done / total),
    )
    const secondBoard = findBoard(second.meta.controller, second.info.mcu, second.info.usable_pins)
    const choice = chooseLink(wires, info.mcu, second.info.mcu, (half, pin) =>
      half === 'a' ? `${pinLabel(board.value, pin)} on this half` : `${pinLabel(secondBoard, pin)} on the other half`,
    )
    if (!choice.ok) {
      cable.state = 'failed'
      cable.problem = choice.problem
      return
    }
    props.session.cable = choice.a
    second.cable = choice.b
    link.value = choice.a
    pairUp(props.session, second)
    // A second half that is set up already gets its side of the cable now.
    const split = second.hardware.split
    if (isConfigured(second) && !second.setup && split !== 'None') {
      const config = { Uart: choice.b }
      await setHardware(second, { ...second.hardware, split: 'Central' in split ? { Central: config } : { Peripheral: config } })
    }
    cable.state = 'found'
    cable.byHand = false
  } catch (error) {
    cable.state = 'failed'
    const text = error instanceof Error ? error.message : String(error)
    cable.problem = text.includes('could not read the request')
      ? 'One of the halves runs firmware from before it could do this. Update the firmware of both halves, then try again.'
      : text
  }
}

// --- Getting both halves ready -------------------------------------------------
const serial = (s: Session) => s.info.uid.slice(-2).map((byte) => byte.toString(16).padStart(2, '0')).join('')
const nameOf = (s: Session) => s.meta.name || s.demo?.label || `${s.transport.label} …${serial(s)}`
const isUsb = props.session.transport.kind === 'usb'
/** What the guide is busy with: a firmware copy takes a while and must not be started twice. */
const flashing = ref<'idle' | 'this' | 'other' | 'new'>('idle')
const flashProblem = ref('')

async function flash(which: 'this' | 'other' | 'new', work: () => Promise<void>) {
  if (flashing.value !== 'idle') return
  flashing.value = which
  flashProblem.value = ''
  try {
    await work()
    if (which === 'new') toast('Firmware installed. The board shows up here in a moment.')
  } catch (error) {
    flashProblem.value = error instanceof Error ? error.message : String(error)
  }
  flashing.value = 'idle'
}
const updateThis = () => flash('this', () => updateFirmware(props.session))
const updateOther = () => other.value && flash('other', () => updateFirmware(other.value!))
const installNew = (drive?: BootDrive) => flash('new', () => installOnBoard(drive))

/** Both halves are here and their firmware can look for the cable. */
const bothReady = computed(() => !!other.value && canFindCable(props.session) && canFindCable(other.value))

// A second keyboard that is clearly a fresh half is searched without asking.
// One that is in use is only touched when the user says so: it cannot type
// while the search runs.
const searchUnasked = computed(
  () => step.value === 'shape' && shape.value !== 'single' && cable.state === 'idle' && bothReady.value && others.value.length === 1 && (!isConfigured(other.value!) || other.value!.setup || partner.value === other.value),
)
watch(searchUnasked, (now) => now && findCable(), { immediate: true })
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
  // The light chain's pin is driven by the firmware and never part of the key wiring.
  wirablePins(board.value, info.usable_pins).filter(
    (pin) => !reserved.has(pin) && !linkPins.value.includes(pin) && pin !== props.session.lighting.pin,
  ),
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

/** The pins the keys pressed so far were found on. */
let probed = ''
async function startProbing() {
  // Back from a side trip that changed nothing: the keys found so far stay.
  const pins = candidates.value.join(',')
  if (pins !== probed) {
    collector.value = reactive(new KeyCollector())
    encoders.splice(0)
  }
  probed = pins
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
onMounted(() => step.value === 'keys' && startProbing())
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

/** Every key is pressed: a wiring that makes sense is put to work at once. */
function allPressed() {
  if (result.value.ok) apply()
  else go('result')
}

/** From the tester back to pressing keys, to add one that was missed. */
function pressAgain() {
  probed = candidates.value.join(',')
  go('keys')
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
  resume.delete(props.session.id)
  props.session.setup = false
  if (next === 'partner' && partner.value) app.currentId = partner.value.id
  // The cable only starts working with the firmware's next start.
  const second = partner.value
  if (second && !second.setup && second.unsaved && isConfigured(second)) await attempt(() => save(second))
  for (const half of [props.session, second]) {
    if (half && !half.setup && needsRestart(half)) await attempt(() => restart(half))
  }
}

const canContinue = computed(() => {
  if (step.value === 'shape') {
    if (shape.value === 'single') return true
    // A split goes on once its cable is known: found, or picked by hand.
    return cable.state !== 'searching' && flashing.value === 'idle' && !!link.value && (cable.state === 'found' || cable.byHand)
  }
  if (step.value === 'keys') return collector.value.keys.length > 0 && !knobFinder.value
  return true
})

/** Whether the explanation of how keys are found is open. */
const explaining = ref(false)
const showWiring = ref(false)

function next() {
  if (step.value === 'keys') allPressed()
  else go('keys')
}

function back() {
  if (step.value === 'shape' && fromKeys && !canContinue.value) shape.value = 'single'
  if (step.value === 'shape' && !fromKeys) app.currentId = undefined
  else go('keys')
}
</script>

<template>
  <div class="wizard">
    <header class="top row">
      <span class="chip accent">{{ step === 'finish' ? 'Step 2 of 2 · Try it' : 'Step 1 of 2 · Find the keys' }}</span>
      <span class="grow" />
      <span class="muted small">{{ session.transport.label }} · {{ board.name }}</span>
    </header>

    <Transition :name="direction === 'forward' ? 'slide' : 'slide-back'" mode="out-in">
      <!-- 2 ------------------------------------------------------------------>
      <section v-if="step === 'board'" key="board" class="page two">
        <div class="col text">
          <span class="chip accent">Your controller</span>
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
          <span class="chip accent">Shape</span>
          <h1>One piece, or two halves?</h1>
          <div class="choices col">
            <label class="choice" :class="{ on: shape === 'single' }">
              <input v-model="shape" type="radio" value="single" data-test="shape-single" />
              <div><strong>One piece</strong>
                <div class="muted small">A macro pad or a normal keyboard with a single controller.</div></div>
            </label>
            <label class="choice" :class="{ on: shape === 'central' }">
              <input v-model="shape" type="radio" value="central" data-test="shape-central" />
              <div><strong>The half that plugs into the computer</strong> <span class="chip" data-test="experimental">experimental</span>
                <div class="muted small">It keeps the keymap and listens to the other half over the cable.</div></div>
            </label>
            <label class="choice" :class="{ on: shape === 'peripheral' }">
              <input v-model="shape" type="radio" value="peripheral" data-test="shape-peripheral" />
              <div><strong>The other half</strong> <span class="chip">experimental</span>
                <div class="muted small">It only reports its keys. Set this one up over its own USB port too.</div></div>
            </label>
          </div>
          <Transition name="rise">
            <div v-if="shape !== 'single'" class="col">
              <p class="note warn">
                Split keyboards are experimental: they work in testing, but have had little use on real boards.
                Keyboards in one piece are the supported kind for now.
              </p>
              <ol class="guide" data-test="split-guide">
                <li class="done">
                  <span class="mark" />
                  <div class="col">
                    <strong>This half is connected</strong>
                    <span class="muted small">{{ nameOf(session) }}, firmware {{ info.firmware.join('.') }}</span>
                    <div v-if="!canFindCable(session)" class="note warn col" data-test="this-old">
                      <span>Its firmware is from before JMK could find the cable by itself.</span>
                      <div v-if="desktop && isUsb" class="row wrap">
                        <button class="btn small primary" :disabled="flashing !== 'idle'" data-test="update-this" @click="updateThis">
                          {{ flashing === 'this' ? 'Updating… it comes back by itself' : 'Update this half' }}
                        </button>
                      </div>
                      <span v-else class="small">Update it under Hardware, or choose the pins by hand below.</span>
                    </div>
                  </div>
                </li>

                <li :class="other ? 'done' : 'now'" data-test="guide-other">
                  <span class="mark" />
                  <div class="col">
                    <template v-if="other">
                      <strong>The other half is connected</strong>
                      <span class="muted small">{{ nameOf(other) }}, firmware {{ other.info.firmware.join('.') }}</span>
                      <label v-if="others.length > 1" class="field small">
                        Several keyboards are open. Which one is the other half?
                        <select v-model="otherId" class="input" data-test="cable-other">
                          <option v-for="s in others" :key="s.id" :value="s.id">{{ nameOf(s) }}</option>
                        </select>
                      </label>
                      <div v-if="!canFindCable(other)" class="note warn col" data-test="other-old">
                        <span>Its firmware is from before JMK could find the cable by itself.</span>
                        <div v-if="desktop && isUsb" class="row wrap">
                          <button class="btn small primary" :disabled="flashing !== 'idle'" data-test="update-other" @click="updateOther">
                            {{ flashing === 'other' ? 'Updating… it comes back by itself' : 'Update the other half' }}
                          </button>
                        </div>
                        <span v-else class="small">Update it under its Hardware tab, or choose the pins by hand below.</span>
                      </div>
                    </template>
                    <template v-else>
                      <strong>Plug in the other half too</strong>
                      <span class="muted small">
                        Connect the cable between the two halves first, while neither half has power, then give
                        each half its own USB cable to the computer. It appears here by itself; there is nothing
                        to click.
                      </span>
                      <span class="waiting small" data-test="other-waiting">Waiting for the other half…</span>
                      <div v-for="drive in bootDrives" :key="drive.path" class="note col" data-test="boot-drive">
                        <span><strong>A board in update mode is attached</strong> ({{ drive.model }}). It has no JMK firmware running yet.</span>
                        <div class="row wrap">
                          <button class="btn small primary" :disabled="flashing !== 'idle'" data-test="install-new" @click="installNew(drive)">
                            {{ flashing === 'new' ? 'Copying the firmware…' : 'Install JMK on it' }}
                          </button>
                        </div>
                      </div>
                      <span v-if="!bootDrives.length" class="muted small">
                        <template v-if="desktop">
                          Is it a new board without JMK? Unplug it completely, the cable between the halves too,
                          since the board must be without power. Hold its BOOT (or BOOTSEL) button, plug in its USB
                          cable and let go. It is then offered here for installing. Use a USB cable that carries
                          data; many carry power only. Boards with a single button (the 0xCB Helios, for one)
                          work differently: plug the board in, hold the button for a full second, let go.
                        </template>
                        <template v-else>
                          Is it a new board without JMK? Hold its BOOT button while plugging it in and copy the JMK
                          firmware file onto the drive that appears. The desktop app does this for you.
                        </template>
                      </span>
                    </template>
                    <p v-if="flashProblem" class="note warn small" data-test="flash-problem">{{ flashProblem }}</p>
                  </div>
                </li>

                <li :class="cable.state === 'found' ? 'done' : other ? 'now' : ''" data-test="guide-cable">
                  <span class="mark" />
                  <div class="col">
                    <strong>{{ cable.state === 'found' ? 'The cable between the halves is found' : 'Find the cable between the halves' }}</strong>
                    <div v-if="cable.state === 'searching'" class="col" data-test="cable-searching">
                      <span class="muted small">Looking at every pin… neither half types for a few seconds.</span>
                      <progress :value="cable.progress" max="1" />
                    </div>
                    <span v-else-if="cable.state === 'found' && session.cable" class="muted small" data-test="cable-found">
                      This half sends on {{ pinLabel(board, session.cable.tx) }} and receives on
                      {{ pinLabel(board, session.cable.rx) }}. The other half is set to match.
                    </span>
                    <template v-else>
                      <span v-if="!other" class="muted small" data-test="cable-need-both">
                        Done by itself once both halves are here. You do not need to know which pins the cable is on.
                      </span>
                      <span v-else-if="!bothReady" class="muted small">Needs current firmware on both halves; see above.</span>
                      <div v-else class="row wrap">
                        <button class="btn small primary" data-test="find-cable" @click="findCable">
                          Find the cable to {{ nameOf(other) }}
                        </button>
                        <span class="muted small">That keyboard cannot type for a few seconds while this runs.</span>
                      </div>
                      <p v-if="cable.state === 'failed'" class="note warn small" data-test="cable-problem">{{ cable.problem }}</p>
                    </template>
                  </div>
                </li>
              </ol>
              <div class="row wrap">
                <button v-if="cable.state === 'found'" class="btn small ghost" data-test="find-cable-again" @click="findCable">Look again</button>
                <button v-if="cable.state !== 'searching'" class="btn small ghost" data-test="cable-by-hand" @click="cable.byHand = !cable.byHand">
                  {{ cable.byHand ? 'Hide the pin list' : 'I know the pins: choose them by hand' }}
                </button>
              </div>
              <label v-if="cable.byHand" class="field">
                <select v-model="link" class="input" data-test="link">
                  <option v-for="pair in linkChoices" :key="pair.tx + '-' + pair.rx" :value="pair">
                    sends on {{ pinLabel(board, pair.tx) }}, receives on {{ pinLabel(board, pair.rx) }}
                  </option>
                </select>
                <span class="muted small">
                  Each half's sending pin must be wired to the other half's receiving pin, plus ground and power.
                  <template v-if="!linkPairs.length">No free pin pair on this board can carry a serial cable.</template>
                </span>
              </label>
            </div>
          </Transition>
        </div>
        <BoardDiagram :board="board" :roles="roles" />
      </section>

      <!-- Keys --------------------------------------------------------------->
      <section v-else-if="step === 'keys'" key="keys" class="page two">
        <div class="col text">
          <h1>Press every key once.</h1>
          <p class="muted lead">
            Any order. That is the whole setup: the app watches the pins light up and works out the wiring.
          </p>

          <div class="tally row">
            <strong class="count" data-test="count">
              <Transition name="tick" mode="out-in"><span :key="collector.keys.length">{{ collector.keys.length }}</span></Transition>
              {{ collector.keys.length === 1 ? 'key' : 'keys' }}
            </strong>
            <span v-if="collector.last !== undefined" class="muted mono">{{ describeKey(collector.keys[collector.last]) }}</span>
            <span v-else class="waiting">Waiting for the first key…</span>
          </div>
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
          </div>
          <Transition name="rise">
            <div v-if="collector.confused" class="note warn">
              That looked like more than one key at the same instant. Press them one after the other.
            </div>
          </Transition>
          <Transition name="rise">
            <div v-if="collector.stuck.length" class="note warn">
              Some pins were already connected before you pressed anything, so they are ignored. If that is not a
              stuck key, <button class="link" @click="go('board')">mark them as protected</button>.
            </div>
          </Transition>
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
          </div>

          <!-- Everything that is not needed for an ordinary board waits down here. -->
          <div class="asides col">
            <button v-if="!knobFinder && encoders.length < LIMITS.encoders" class="aside" data-test="find-knob" @click="findKnob">
              <span class="plus">+</span> It has a rotary knob
            </button>
            <button class="aside" data-test="open-board" @click="go('board')">
              <span class="plus">›</span>
              <span>Set up as <strong>{{ board.name }}</strong>. Another board, or a display or LEDs soldered on?</span>
            </button>
            <button class="aside" data-test="open-shape" @click="go('shape')">
              <span class="plus">›</span>
              <span>It is one half of a split keyboard <span class="chip">experimental</span></span>
            </button>
            <button class="aside" data-test="explain" @click="explaining = !explaining">
              <span class="plus">?</span> {{ explaining ? 'Back to the board' : 'How does pressing keys tell it the wiring?' }}
            </button>
          </div>
        </div>
        <Transition name="swap" mode="out-in">
          <div v-if="explaining" key="why" class="col visuals">
            <MatrixExplainer />
            <p class="muted small hint">
              A controller has fewer pins than a keyboard has keys, which is why keys are wired in a grid like this.
              While you press yours, only weak test currents are used, and only on the yellow pins.
            </p>
          </div>
          <div v-else key="board" class="col visuals">
            <BoardDiagram :board="board" :roles="roles" :active="collector.active" />
          </div>
        </Transition>
      </section>

      <!-- Only when the presses did not add up to a wiring ------------------->
      <section v-else-if="step === 'result'" key="result" class="page two">
        <div class="col text">
          <span class="chip accent">Not understood yet</span>
          <h1>Something is unusual.</h1>
          <div class="note warn">{{ result.ok ? '' : result.problem }}</div>
          <p class="muted">Go back to press the keys again, or fix the wiring and retry.</p>
        </div>
        <div class="col visuals">
          <BoardDiagram :board="board" :roles="roles" />
        </div>
      </section>

      <!-- Finish ------------------------------------------------------------->
      <section v-else key="finish" class="page finish col">
        <span class="chip signal">It works</span>
        <h1 v-if="shape === 'peripheral'">This half is ready.</h1>
        <h1 v-else>Press a key. It already types.</h1>
        <p class="muted center">
          <template v-if="shape === 'peripheral'">
            It reports its {{ keyCount }} keys over the cable. The half that plugs into the computer decides what
            they do. After saving, both halves restart so the cable starts working; then this half's own USB cable
            can come out if the cable between the halves also carries power.
          </template>
          <template v-else>
            Every key got a letter for now. Next you arrange the keys the way they sit on your desk and choose
            what each one does.
            <template v-if="shape === 'central' && partner?.setup"> First, the other half needs the same short setup.</template>
            <template v-else-if="shape === 'central' && !partner">
              The other half needs the same short setup: plug it in and it opens by itself.
            </template>
          </template>
        </p>
        <div class="tester">
          <KeyboardView :layout="session.layout" :legend="keyLegend" :pressed="session.pressed" :knobs="knobPositions(session)" data-test="tester" />
        </div>
        <div class="row save">
          <input v-model="name" class="input name" :maxlength="LIMITS.nameLength" placeholder="Name your keyboard" data-test="name" @keydown.enter="finish('editor')" />
          <button class="btn primary" data-test="finish" @click="finish('editor')">Save and continue</button>
          <button v-if="partner?.setup" class="btn" data-test="finish-partner" @click="finish('partner')">
            Save, then set up the {{ partner.demo?.label.toLowerCase() ?? 'other half' }}
          </button>
        </div>
        <div class="col wiring">
          <div class="row wrap center-row">
            <button class="aside" data-test="show-wiring" @click="showWiring = !showWiring">
              <span class="plus">?</span> {{ showWiring ? 'Hide the wiring' : 'What the app found out about the wiring' }}
            </button>
            <button class="aside" data-test="press-again" @click="pressAgain"><span class="plus">‹</span> A key is missing: press them again</button>
          </div>
          <Transition name="rise">
            <div v-if="showWiring && result.ok" class="card found-out col">
              <ul class="notes">
                <li v-for="note in result.wiring.notes" :key="note">{{ note }}</li>
                <li v-if="encoders.length">{{ encoders.length }} rotary knob{{ encoders.length > 1 ? 's' : '' }}.</li>
                <li v-if="shape !== 'single'">Cable to the other half on the pink pins.</li>
              </ul>
              <MatrixExplainer v-if="scan" :driven="scan.driven" :sensed="scan.sensed" :diodes="scan.diodes" :held="[]" />
            </div>
          </Transition>
        </div>
      </section>
    </Transition>

    <footer v-if="step !== 'finish'" class="bottom row">
      <button v-if="step !== 'keys'" class="btn ghost" data-test="back" @click="back">Back</button>
      <span class="grow" />
      <button class="btn primary" :disabled="!canContinue || applying" data-test="next" @click="next">
        {{ applying ? 'Setting up…' : step === 'keys' ? "That's all of them" : step === 'result' ? 'Press the keys again' : 'Continue' }}
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

.lead {
  font-size: 1.08rem;
}

.tally {
  gap: 14px;
  align-items: baseline;
  margin-top: 10px;
}

.count {
  font-size: 1.5rem;
  letter-spacing: -0.01em;
}

.count span {
  display: inline-block;
  min-width: 0.7em;
  color: var(--accent);
}

.tick-enter-active {
  transition:
    opacity 0.2s var(--ease),
    transform 0.25s cubic-bezier(0.3, 1.6, 0.5, 1);
}
.tick-leave-active {
  transition: opacity 0.06s;
}
.tick-enter-from {
  opacity: 0;
  transform: translateY(8px) scale(0.8);
}
.tick-leave-to {
  opacity: 0;
}

.asides {
  align-self: stretch;
  gap: 2px;
  margin-top: 18px;
  padding-top: 14px;
  border-top: 1px solid var(--line);
}

.aside {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  margin-left: -8px;
  border: none;
  border-radius: 8px;
  background: none;
  color: var(--muted);
  font-size: 0.9rem;
  text-align: left;
  cursor: pointer;
  transition:
    color 0.15s,
    background 0.15s,
    transform 0.15s var(--ease);
}

.aside strong {
  color: var(--text);
  font-weight: 600;
}

.aside:hover {
  color: var(--text);
  background: var(--panel-2);
  transform: translateX(3px);
}

.plus {
  flex: none;
  width: 18px;
  text-align: center;
  color: var(--accent);
  font-weight: 700;
}

.link {
  padding: 0;
  border: none;
  background: none;
  color: var(--accent);
  text-decoration: underline;
  cursor: pointer;
}

.hint {
  max-width: 420px;
  text-align: center;
}

.swap-enter-active {
  transition:
    opacity 0.3s var(--ease),
    transform 0.3s var(--ease);
}
.swap-leave-active {
  transition: opacity 0.12s;
}
.swap-enter-from {
  opacity: 0;
  transform: scale(0.96);
}
.swap-leave-to {
  opacity: 0;
}

.save {
  flex-wrap: wrap;
  justify-content: center;
}

.name {
  width: 220px;
  text-align: center;
}

.wiring {
  align-items: center;
  gap: 12px;
}

.center-row {
  justify-content: center;
  gap: 4px 18px;
}

.center-row .aside {
  margin-left: 0;
}

.found-out {
  align-items: center;
  text-align: left;
  max-width: 560px;
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
  min-height: 48px;
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
  background: var(--accent);
  color: var(--accent-ink);
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

.visuals {
  align-items: center;
}

.finish {
  align-items: center;
  justify-content: safe center;
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

.wrap {
  flex-wrap: wrap;
}

progress {
  width: 100%;
  accent-color: var(--signal);
}

.guide {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.guide li {
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr);
  gap: 12px;
  align-items: start;
}

.guide .col {
  gap: 6px;
}

.mark {
  width: 22px;
  height: 22px;
  margin-top: 1px;
  border-radius: 50%;
  border: 2px solid var(--line);
  display: grid;
  place-items: center;
  font-size: 13px;
  font-weight: 700;
}

.guide li.now .mark {
  border-color: var(--accent);
  box-shadow: 0 0 0 4px var(--accent-soft);
}

.guide li.done .mark {
  border-color: var(--signal);
  background: var(--signal);
  color: #06201d;
}

.guide li.done .mark::after {
  content: '✓';
}

.guide li:not(.done):not(.now) > .col {
  opacity: 0.6;
}

.waiting {
  color: var(--accent);
  animation: breathe 1.6s ease-in-out infinite;
}

@keyframes breathe {
  50% {
    opacity: 0.45;
  }
}
</style>
