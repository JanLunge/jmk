<script lang="ts">
// Where each keyboard's setup had got to, so it carries on there when the
// keyboard drops off for a firmware update and comes back.
const resume = new Map<string, { step: 'board' | 'shape' | 'keys'; boardId: string }>()
/** Halves whose setup runs from the other half's wizard: their id, and the id of the half showing it. */
const led = new Map<string, string>()

interface HalfSetup {
  collector: KeyCollector
  encoders: EncoderConfig[]
  finder?: KnobFinder
}

/**
 * What a setup has found out so far, by the id of the board showing it. A
 * board that is unplugged in the middle comes back to its keys.
 */
interface Kept {
  setups: Map<string, HalfSetup>
  /** The pins the keys were found on. */
  probed: string
  firstId?: string
  secondId?: string
  cables: Map<string, { tx: Pin; rx: Pin }>
  /** The half of each key found, oldest first, for taking the last one back. */
  history: string[]
}
const kept = new Map<string, Kept>()
</script>

<script setup lang="ts">
// First-time setup: from an unconfigured board to a working keyboard. Two
// boards joined by a cable are set up together as the halves of a split.
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import BoardDiagram, { type PinRole } from '../components/BoardDiagram.vue'
import BoardThumb from '../components/BoardThumb.vue'
import SetupGuide from '../components/SetupGuide.vue'
import MatrixExplainer from '../components/MatrixExplainer.vue'
import { app, attempt, benchOf, pairUp, partnerOf, relink, toast } from '../lib/app'
import { boardsFor, findBoard, genericBoard, pinLabel, pinsOfAnyShape, shapeFromPins, wirablePins } from '../lib/boards'
import { desktop, type BootDrive } from '../lib/desktop'
import { localPositions } from '../lib/definition'
import { arrivals as allArrivals, bootDrives, installArrival, installOnBoard, updateFirmware } from '../lib/firmware'
import SplitPicture, { type PictureBoard } from '../components/SplitPicture.vue'
import { chooseLink, findWires } from '../lib/cable'
import { KeyCollector, KnobFinder } from '../lib/collector'
import { inferWiring, type KeySignature } from '../lib/infer'
import { bounds, hasKey, UNIT } from '../lib/layout'
import type { Action, Contact, EncoderConfig, HardwareConfig, KeyGeometry, Pin } from '../lib/protocol'
import { LIMITS } from '../lib/protocol'
import { isConfigured, linkPins, needsRestart, restart, save, setHardware, setLayout, setMeta, SPLIT_PROTOCOL, starterAction, starterLayout, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

// Pressing the keys is the setup. The controller and the split pages are
// side trips for boards that need them; 'result' only shows when the
// wiring could not be read.
const STEPS = ['board', 'shape', 'keys', 'result'] as const
type Step = (typeof STEPS)[number]
const resumed = resume.get(props.session.id)
const mine: Kept = kept.get(props.session.id) ?? { setups: reactive(new Map()), probed: '', cables: new Map(), history: [] }
kept.set(props.session.id, mine)
const step = ref<Step>(resumed?.step ?? 'keys')
const stepIndex = computed(() => STEPS.indexOf(step.value))
const direction = ref<'forward' | 'back'>('forward')

function go(to: Step) {
  direction.value = STEPS.indexOf(to) > stepIndex.value ? 'forward' : 'back'
  step.value = to
}

// --- Board -----------------------------------------------------------------
const info = props.session.info
const boards = computed(() => [...boardsFor(info.mcu), genericBoard(info.mcu, info.usable_pins)])
/**
 * A chip that comes on boards of more than one shape does not say which it
 * is on. Until the pins in use tell, the smaller shape is drawn and the
 * pins of every shape are looked at.
 */
const AUTO = 'auto'
const shapes = boardsFor(info.mcu)
const boardId = ref(resumed?.boardId || props.session.meta.controller || (shapes.length > 1 ? AUTO : boards.value[0].id))
const usedPins = computed(() =>
  [...mine.setups.values()].flatMap((setup) => [
    ...setup.collector.keys.flatMap((key) => (key.kind === 'link' ? [key.driven, key.sensed] : [key.pin])),
    ...setup.encoders.flatMap((encoder) => [encoder.a, encoder.b]),
  ]),
)
const settledShape = computed(() => shapeFromPins(usedPins.value))
const resolvedId = computed(() => (boardId.value === AUTO ? (settledShape.value ?? shapes[shapes.length - 1].id) : boardId.value))
/** Both halves of a split are taken to be built on the same controller. */
const boardOf = (session: Session) => findBoard(resolvedId.value, session.info.mcu, session.info.usable_pins)
/** Pins of a board that could carry wiring. */
const reachable = (session: Session) =>
  boardId.value === AUTO ? pinsOfAnyShape(session.info.mcu, session.info.usable_pins) : wirablePins(boardOf(session), session.info.usable_pins)
const board = computed(() => boardOf(props.session))
const boardName = computed(() => (boardId.value === AUTO && !settledShape.value ? `${info.mcu === 'Nrf52840' ? 'nRF52840' : 'RP2040'} controller` : board.value.name))
const reserved = reactive(new Set<Pin>(props.session.hardware.reserved_pins))

function toggleReserved(pin: Pin) {
  if (reserved.has(pin)) reserved.delete(pin)
  else reserved.add(pin)
}

watch(
  [step, boardId],
  () => {
    const at = step.value === 'board' || step.value === 'shape' ? step.value : 'keys'
    resume.set(props.session.id, { step: at, boardId: boardId.value })
  },
  { immediate: true },
)

// --- One piece or two halves ---------------------------------------------------
const serial = (s: Session) => s.info.uid.slice(-2).map((byte) => byte.toString(16).padStart(2, '0')).join('')
const nameOf = (s: Session) => s.meta.name || s.demo?.label || `${s.transport.label} …${serial(s)}`
const isUsb = props.session.transport.kind === 'usb'
const canSplit = (s: Session) => s.info.protocol >= SPLIT_PROTOCOL
/** The cable pins known for a half: just found, or set up before. */
const cableOf = (s: Session) => s.cable ?? linkPins(s.hardware)
/** A board that is new or in its setup: looking for a cable on it interrupts nothing. */
const isFresh = (s: Session) => !isConfigured(s) || s.setup

/** Other open keyboards that could be the second half. */
const others = computed(() =>
  app.sessions.filter((s) => {
    if (s === props.session || s.closed || s.transport.kind !== props.session.transport.kind) return false
    // Virtual boards belong together only if they lie on the same bench.
    if (benchOf(s) !== benchOf(props.session)) return false
    // A half that another board's setup has taken is not free.
    const leader = led.get(s.id)
    return leader === undefined || leader === props.session.id
  }),
)
const otherId = ref<string>()
const other = computed(() => others.value.find((s) => s.id === otherId.value) ?? partnerOf(props.session) ?? others.value[0])

/** The other half, once a cable to it is known. With it the setup covers both. */
const secondId = ref(mine.secondId)
watch(secondId, (id) => (mine.secondId = id))
/** Undefined while that half is unplugged; it rejoins when it is back. */
const second = computed(() => app.sessions.find((s) => s.id === secondId.value && !s.closed))
/** This is the setup of a split keyboard, whether or not both halves are here right now. */
const intended = computed(() => !!secondId.value)
/** The user said this board is to be set up alone, whatever it was before. */
const alone = ref(false)
const cable = reactive({ state: 'idle' as 'idle' | 'searching' | 'found' | 'none' | 'failed', progress: 0, problem: '' })
const halves = computed<Session[]>(() => (second.value ? [props.session, second.value] : [props.session]))
const isSplit = computed(() => halves.value.length === 2)
/** A half of a split keyboard whose other half is not plugged into the computer. */
const lonelyHalf = computed(
  () => !isSplit.value && !alone.value && (intended.value || props.session.hardware.split !== 'None') && cable.state !== 'searching',
)

function adopt(half: Session) {
  for (const each of [props.session, half]) if (each.cable) mine.cables.set(each.id, each.cable)
  secondId.value = half.id
  led.set(half.id, props.session.id)
  pairUp(props.session, half)
  cable.state = 'found'
}

function setUpAlone() {
  if (secondId.value) led.delete(secondId.value)
  secondId.value = undefined
  alone.value = true
  cable.state = 'idle'
}

/** Pins of a half that a cable wire could be on. */
function cablePins(session: Session): Pin[] {
  const taken = new Set<Pin>(reserved)
  if (session.lighting.pin !== null) taken.add(session.lighting.pin)
  return reachable(session).filter((pin) => !taken.has(pin))
}

async function findCable(quiet = false) {
  const candidate = other.value
  if (!candidate || cable.state === 'searching') return
  cable.state = 'searching'
  cable.progress = 0
  cable.problem = ''
  alone.value = false
  const wasProbing = probing
  await stopProbing()
  try {
    const wires = await findWires(props.session, candidate, cablePins(props.session), cablePins(candidate), undefined, (done, total) => (cable.progress = done / total))
    const choice = chooseLink(wires, info.mcu, candidate.info.mcu, (half, pin) =>
      half === 'a' ? `${pinLabel(board.value, pin)} on this board` : `${pinLabel(boardOf(candidate), pin)} on the other board`,
    )
    if (choice.ok) {
      props.session.cable = choice.a
      candidate.cable = choice.b
      adopt(candidate)
      // Let the tick on the cable be seen before the keys take over.
      found.value = true
      setTimeout(() => (found.value = false), 1400)
    } else if (wires.length === 0 && quiet) {
      // Two boards that have nothing to do with each other.
      cable.state = 'none'
    } else {
      cable.state = 'failed'
      cable.problem = choice.problem
    }
  } catch (error) {
    cable.state = 'failed'
    const text = error instanceof Error ? error.message : String(error)
    cable.problem = text.includes('could not read the request')
      ? 'One of the boards runs older firmware that cannot do this. Update the firmware of both, then try again.'
      : text
  }
  if (wasProbing && step.value === 'keys' && !probing) await startProbing()
}

/** The cable was found a moment ago. */
const found = ref(false)
/**
 * Two fresh boards are about to be checked for a cable: the search starts
 * once this page is up, and the page opens on it rather than on the keys.
 */
const aboutToSearch = ref(false)

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
/** Boards without firmware that could be this keyboard's other half: on the same bench if it is virtual, on USB if it is real. */
const arrivals = computed(() => {
  const bench = benchOf(props.session)
  return allArrivals.value.filter((arrival) => (bench ? arrival.virtual?.bench === bench : !arrival.virtual))
})
const installNew = (drive?: BootDrive) => flash('new', () => installOnBoard(drive))
/** Boards in update mode, the virtual ones included. */
const installWaiting = () => flash('new', () => Promise.all(arrivals.value.map(installArrival)).then(() => {}))

/** The boards and the cable as a picture, once there is more to this than one board. */
const picture = computed<{ boards: PictureBoard[]; cable: 'idle' | 'searching' | 'found' | 'none' } | undefined>(() => {
  const arrival = arrivals.value[0]
  const near = other.value && !other.value.closed ? other.value : undefined
  const partner = second.value ?? (alone.value || intended.value ? undefined : near)
  if (!intended.value && !partner && !arrival && cable.state !== 'searching') return undefined
  const here: PictureBoard = {
    label: isSplit.value && firstId.value ? sideName(props.session).toLowerCase() : 'this board',
    state: canSplit(props.session) ? 'ready' : 'old',
  }
  const there: PictureBoard = partner
    ? { label: isSplit.value && firstId.value ? sideName(partner).toLowerCase() : 'the other board', state: canSplit(partner) ? 'ready' : 'old' }
    : intended.value
      ? { label: 'the other half', state: 'unplugged', note: 'unplugged · its keys are kept' }
      : arrival
        ? { label: 'new board', state: arrival.progress !== undefined || flashing.value === 'new' ? 'installing' : 'new', progress: arrival.progress }
        : { label: 'the other board', state: 'absent' }
  const state = isSplit.value || intended.value ? 'found' : cable.state === 'searching' ? 'searching' : cable.state === 'idle' ? 'idle' : 'none'
  return { boards: [here, there], cable: state }
})

/** Both boards are here and their firmware knows about halves. */
const bothReady = computed(() => !!other.value && canSplit(props.session) && canSplit(other.value))

// A second board that is clearly new is checked for a cable without asking,
// as long as no key has been pressed yet. A keyboard that is in use is only
// touched when the user says so: it cannot type while the search runs.
const searchUnasked = computed(
  () =>
    !isSplit.value &&
    !alone.value &&
    cable.state === 'idle' &&
    bothReady.value &&
    others.value.length === 1 &&
    isFresh(other.value!) &&
    (step.value === 'shape' || step.value === 'keys'),
)

// --- Finding the keys --------------------------------------------------------
const setups = mine.setups
const ensure = (half: Session) => setups.has(half.id) || setups.set(half.id, { collector: new KeyCollector(), encoders: [] })
const setupOf = (half: Session): HalfSetup => setups.get(half.id) ?? { collector: new KeyCollector(), encoders: [] }
watch(halves, (list) => list.forEach(ensure), { immediate: true })
const keyTotal = computed(() => halves.value.reduce((sum, half) => sum + setupOf(half).collector.keys.length, 0))
const findingKnob = computed(() => halves.value.some((half) => setupOf(half).finder))
const bump = ref(0)
/** The half whose key was pressed first: its keys come first and are drawn on the left. */
const firstId = ref(mine.firstId)
watch(firstId, (id) => (mine.firstId = id))
/** The half that was pressed last; its board is the one drawn. */
const shownId = ref(props.session.id)
const ordered = computed(() => {
  const list = halves.value
  return list.length === 2 && firstId.value === list[1].id ? [list[1], list[0]] : list
})
const shown = computed(() => halves.value.find((half) => half.id === shownId.value) ?? props.session)
const sideName = (half: Session) => (!isSplit.value ? '' : ordered.value[0] === half ? 'Left half' : 'Right half')

/** The half that is unplugged right now, as far as its keys are known. */
const awayKeys = computed(() => (intended.value && !second.value ? (setups.get(secondId.value!)?.collector.keys.length ?? 0) : undefined))
/** Halves usually have the same number of keys; a difference may be a key not pressed yet. */
const unequal = computed(() => {
  if (!isSplit.value) return undefined
  const [left, right] = ordered.value.map((half) => setupOf(half).collector.keys.length)
  return left !== right && Math.max(left, right) >= 4 ? { left, right, more: Math.abs(left - right), on: left > right ? 'right' : 'left' } : undefined
})

function swapSides() {
  if (ordered.value.length === 2) firstId.value = ordered.value[1].id
}

/** Take back the key found last. */
function undoLast() {
  for (let id = mine.history.pop(); id; id = mine.history.pop()) {
    const collector = setups.get(id)?.collector
    if (!collector?.keys.length) continue
    collector.remove(collector.keys.length - 1)
    break
  }
  if (keyTotal.value === 0) firstId.value = undefined
  bump.value++
}

const candidatesOf = (half: Session) => {
  const link = isSplit.value ? cableOf(half) : undefined
  // The light chain's pin is driven by the firmware and never part of the key wiring.
  return reachable(half).filter((pin) => !reserved.has(pin) && pin !== link?.tx && pin !== link?.rx && pin !== half.lighting.pin)
}

function onProbe(half: Session, contacts: Contact[]) {
  const setup = setupOf(half)
  if (setup.finder) {
    const pins = setup.finder.update(contacts)
    if (pins) {
      setup.encoders.push({ a: pins[0], b: pins[1], steps_per_detent: 4 })
      // Turning a knob can look like key presses on its pins; drop those.
      const keys = setup.collector.keys
      for (let i = keys.length - 1; i >= 0; i--) {
        const key = keys[i]
        if (key.kind !== 'link' && pins.includes(key.pin)) setup.collector.remove(i)
      }
      halves.value.forEach((each) => (setupOf(each).finder = undefined))
    }
    return
  }
  if (findingKnob.value) return
  const encoderPins = new Set(setup.encoders.flatMap((e) => [e.a, e.b]))
  const relevant = contacts.filter((c) => !('Ground' in c && encoderPins.has(c.Ground)))
  const before = setup.collector.last
  const count = setup.collector.keys.length
  setup.collector.update(relevant)
  if (setup.collector.keys.length > count || setup.collector.last !== before) {
    firstId.value ??= half.id
    shownId.value = half.id
  }
  if (setup.collector.keys.length > count) mine.history.push(half.id)
  bump.value++
}

let probing = false
const allCandidates = () => halves.value.map((half) => half.id + ':' + candidatesOf(half).join(',')).join(';')
async function startProbing() {
  // The keys found so far stay, except those on pins that are no longer
  // looked at (the cable's, say) and those of a board that left the setup.
  const pins = allCandidates()
  if (pins !== mine.probed) {
    for (const half of halves.value) {
      const looked = new Set(candidatesOf(half))
      const collector = setupOf(half).collector
      for (let index = collector.keys.length - 1; index >= 0; index--) {
        const key = collector.keys[index]
        const used = key.kind === 'link' ? [key.driven, key.sensed] : [key.pin]
        if (!used.every((pin) => looked.has(pin))) collector.remove(index)
      }
    }
    for (const id of [...setups.keys()]) if (id !== props.session.id && id !== secondId.value) setups.delete(id)
    if (keyTotal.value === 0) firstId.value = undefined
  }
  mine.probed = pins
  probing = true
  for (const half of halves.value) {
    ensure(half)
    half.client.onProbe = (contacts) => onProbe(half, contacts)
    await attempt(() => half.client.startProbe(candidatesOf(half)))
  }
}

async function stopProbing() {
  if (!probing) return
  probing = false
  for (const half of halves.value) {
    setupOf(half).finder = undefined
    half.client.onProbe = () => {}
    await half.client.stopProbe().catch(() => {})
  }
}

function findKnob() {
  for (const half of halves.value) {
    const setup = setupOf(half)
    const used = new Set(setup.collector.keys.flatMap((key) => (key.kind === 'link' ? [key.driven, key.sensed] : [key.pin])))
    setup.finder = new KnobFinder(used)
  }
}
const cancelKnob = () => halves.value.forEach((half) => (setupOf(half).finder = undefined))

watch(step, async (now, before) => {
  if (now === 'keys' && before !== 'result') await startProbing()
  if (before === 'keys' && now !== 'result') await stopProbing()
})
watch(searchUnasked, (now) => now && findCable(true))
aboutToSearch.value = searchUnasked.value && !led.has(props.session.id)
// A half that was unplugged keeps its place and its keys. When it is back it
// is a new connection to the same board: tell it again what to look at.
watch(second, async (now, before) => {
  if (!now || now === before) return
  if (!now.cable && mine.cables.has(now.id)) now.cable = mine.cables.get(now.id)
  led.set(now.id, props.session.id)
  if (step.value !== 'keys' || !probing) return
  ensure(now)
  now.client.onProbe = (contacts) => onProbe(now, contacts)
  await attempt(() => now.client.startProbe(candidatesOf(now)))
})

onMounted(async () => {
  // This half's setup runs from the other half's page.
  const leader = app.sessions.find((s) => s.id === led.get(props.session.id) && !s.closed && s.setup)
  if (leader) {
    app.currentId = leader.id
    return
  }
  led.delete(props.session.id)
  if (!props.session.cable && mine.cables.has(props.session.id)) props.session.cable = mine.cables.get(props.session.id)
  // Halves that were paired before, with their cable known, need no search.
  const partner = partnerOf(props.session)
  if (partner && !partner.cable && mine.cables.has(partner.id)) partner.cable = mine.cables.get(partner.id)
  if (partner && !partner.closed && cableOf(partner) && cableOf(props.session) && canSplit(partner) && canSplit(props.session)) adopt(partner)
  else if (searchUnasked.value) {
    aboutToSearch.value = false
    await findCable(true)
  }
  aboutToSearch.value = false
  if (step.value === 'keys' && !probing) await startProbing()
})
onBeforeUnmount(stopProbing)

function describeKey(half: Session, key: KeySignature): string {
  const name = (pin: Pin) => pinLabel(boardOf(half), pin)
  if (key.kind === 'ground') return `${name(key.pin)} to ground`
  if (key.kind === 'supply') return `${name(key.pin)} to supply`
  return `${name(key.driven)} ↔ ${name(key.sensed)}${key.diode ? ' with diode' : ''}`
}

// --- Result ------------------------------------------------------------------
const results = computed(() => ordered.value.map((half) => ({ half, result: inferWiring(setupOf(half).collector.keys) })))
/** The first thing that could not be read, if any. */
const problem = computed(() => {
  for (const { half, result } of results.value) {
    if (!result.ok) return (isSplit.value ? sideName(half) + ': ' : '') + result.problem
  }
  return ''
})
const result = computed(() => results.value[0].result)

/** The wiring of every half, in keymap order. */
const plans = computed(() => {
  let offset = 0
  return results.value.map(({ half, result }) => {
    if (!result.ok) return undefined
    const link = cableOf(half)
    const hardware: HardwareConfig = {
      matrix: result.wiring.matrix,
      reserved_pins: [...reserved],
      debounce_ms: 5,
      encoders: [...setupOf(half).encoders],
      split: isSplit.value && link ? { Half: { link: { Uart: link }, offset } } : 'None',
    }
    const plan = { half, hardware, positions: result.wiring.positions, offset }
    offset += localPositions(hardware)
    return plan
  })
})

const roles = computed(() => {
  const map = new Map<Pin, PinRole>()
  const half = step.value === 'keys' ? shown.value : props.session
  if (step.value === 'keys') candidatesOf(half).forEach((pin) => map.set(pin, 'candidate'))
  reserved.forEach((pin) => map.set(pin, 'reserved'))
  const link = isSplit.value ? cableOf(half) : undefined
  if (link) [link.tx, link.rx].forEach((pin) => map.set(pin, 'link'))
  setupOf(half).encoders.forEach((e) => [e.a, e.b].forEach((pin) => map.set(pin, 'encoder')))
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

const applying = ref(false)
async function apply() {
  const all = plans.value
  if (all.some((plan) => !plan)) return
  applying.value = true
  const done = await attempt(async () => {
    await stopProbing()
    // One picture and one keymap for the whole keyboard: the first half's
    // keys, then the second half's to the right of them.
    const layout: KeyGeometry[] = []
    const actions: Action[] = []
    let letter = 0
    for (const plan of all) {
      const { hardware, positions, offset } = plan!
      const shift = layout.length ? Math.round(bounds(layout.filter(hasKey)).w + 2 * UNIT) : 0
      const own = starterLayout(hardware, positions)
      layout.push(...own.map((key) => (hasKey(key) ? { ...key, x: key.x + shift } : key)))
      // Letters in the order the keys sit in the grid, so the tester shows life at once.
      const sorted = [...positions].sort((a, b) => a - b)
      const mine: Action[] = Array(own.length).fill('Transparent')
      for (const pos of sorted) mine[pos] = starterAction(letter++)
      const firstKnob = own.length - 2 * hardware.encoders.length
      for (const index of hardware.encoders.keys()) {
        mine[firstKnob + 2 * index] = { Basic: { Consumer: 0xea } }
        mine[firstKnob + 2 * index + 1] = { Basic: { Consumer: 0xe9 } }
      }
      while (actions.length < offset) actions.push('Transparent')
      actions.push(...mine)
    }
    for (const plan of all) {
      const { half, hardware } = plan!
      await setHardware(half, hardware)
      await setLayout(half, layout)
      await half.client.setActions(0, 0, actions)
      half.layers[0].splice(0, actions.length, ...actions)
      await setMeta(half, { name: name.value.trim().slice(0, LIMITS.nameLength), controller: board.value.id })
    }
    return true
  })
  // Saved and restarted, the keyboard opens in the editor: that is where it is tried and named.
  if (done) await finish()
  applying.value = false
}

/** Every key is pressed: a wiring that makes sense is put to work at once. */
function allPressed() {
  if (!problem.value) apply()
  else go('result')
}

// --- Finish ------------------------------------------------------------------
const name = ref(props.session.meta.name || (props.session.demo && !props.session.demo.link ? props.session.demo.label : ''))
async function finish() {
  const all = [...ordered.value]
  const done = await attempt(async () => {
    for (const half of all) {
      await setMeta(half, { name: name.value.trim().slice(0, LIMITS.nameLength), controller: board.value.id })
      await save(half)
    }
    return true
  })
  if (!done) return
  for (const half of all) {
    resume.delete(half.id)
    led.delete(half.id)
    kept.delete(half.id)
    half.setup = false
    if (!app.fresh.includes(half.id)) app.fresh.push(half.id)
  }
  relink()
  app.currentId = all[0].id
  // The cable only starts working with the firmware's next start.
  for (const half of all) {
    if (needsRestart(half)) await attempt(() => restart(half))
  }
}

// --- What is on the screen -----------------------------------------------------
const view = computed(() => (step.value === 'keys' && (cable.state === 'searching' || found.value || aboutToSearch.value) ? 'cable' : step.value))
/** Two boards are in play, so finding their cable is a step of its own. */
const splitFlow = computed(
  () => intended.value || view.value === 'cable' || arrivals.value.length > 0 || (!alone.value && !!other.value && !other.value.closed && isFresh(other.value)),
)
const guideSteps = computed(() => (splitFlow.value ? ['Plug in', 'Firmware', 'Find the cable', 'Find the keys'] : ['Plug in', 'Firmware', 'Find the keys']))
const guideAt = computed(() => (view.value === 'cable' ? 2 : guideSteps.value.length - 1))
const tip = computed(() => {
  if (view.value === 'cable') {
    return { title: 'Good to know', text: 'The app tries every pin on both boards to find the two wires of the cable. You never need to know which pins they are on.' }
  }
  if (view.value === 'board') {
    return { title: 'Good to know', text: 'Mark pins that have something other than a key soldered to them. JMK never sends anything to a marked pin.' }
  }
  if (view.value === 'shape') {
    return {
      title: 'From experience',
      text: 'Join the halves with their cable before the USB cables, and do not plug or pull that cable while a half has power: the plug shorts its contacts on the way in.',
    }
  }
  if (view.value === 'result') return undefined
  return {
    title: 'From experience',
    text: 'Go row by row from the top left. A key that does not show up is nearly always a cold solder joint, or a diode the wrong way round.',
  }
})

/** The menu of things most keyboards do not have. */
const menu = ref(false)
/** The whole controller is shown, or the explanation of how keys are found. */
const pins = ref(false)
function showPins(explain: boolean) {
  explaining.value = explain
  pins.value = true
}
/** Pins of the board in front that are not looked at for keys. */
const takenPins = computed(() => {
  const link = isSplit.value ? cableOf(shown.value) : undefined
  return [...reserved, ...(link ? [link.tx, link.rx] : [])]
})
/**
 * Connections that were there before any key was pressed, on pins the user
 * could have wired. Pins that only one shape of the board brings out are
 * busy with the board's own parts on the other shape and say nothing.
 */
const stuckPins = computed(() => {
  const ambiguous = new Set(boardId.value === AUTO ? shapes.flatMap((shape) => shape.internal.map((i) => i.pin)) : [])
  return halves.value.some((half) =>
    setupOf(half).collector.stuck.some((contact) => {
      const on = 'Ground' in contact ? [contact.Ground] : 'Supply' in contact ? [contact.Supply] : [contact.Link.driven, contact.Link.sensed]
      return on.some((pin) => !ambiguous.has(pin))
    }),
  )
})

const canContinue = computed(() => {
  if (cable.state === 'searching' || flashing.value !== 'idle') return false
  if (step.value === 'keys') {
    return !lonelyHalf.value && !findingKnob.value && halves.value.every((half) => setupOf(half).collector.keys.length > 0)
  }
  return true
})

/** Whether the explanation of how keys are found is open. */
const explaining = ref(false)

function next() {
  if (step.value === 'keys') allPressed()
  else go('keys')
}
</script>

<template>
  <div class="wizard">
    <SetupGuide :steps="guideSteps" :current="guideAt" :tip="tip" />
    <div class="stage col">
    <Transition :name="direction === 'forward' ? 'slide' : 'slide-back'" mode="out-in">
      <!-- 2 ------------------------------------------------------------------>
      <section v-if="view === 'board'" key="board" class="page two">
        <div class="col text">
          <span class="chip accent">Your controller</span>
          <h1>Which board is the brain?</h1>
          <div class="choices col">
            <label v-if="shapes.length > 1" class="choice" :class="{ on: boardId === AUTO }">
              <input v-model="boardId" type="radio" :value="AUTO" data-test="board-auto" />
              <div>
                <strong>Work it out from the pins in use</strong>
                <div class="muted small">Boards with this chip come in {{ shapes.length }} shapes and do not say which they are.</div>
              </div>
            </label>
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

      <!-- Two halves ---------------------------------------------------------->
      <section v-else-if="view === 'shape'" key="shape" class="page two">
        <div class="col text">
          <span class="chip accent">Split keyboard</span>
          <h1>Two halves, set up together.</h1>
          <p class="muted">
            Join the halves with their cable and give each its own USB cable to the computer. The app finds the
            cable by itself, and then you press the keys of both halves on one page. Afterwards either half can
            be the one plugged into the computer.
          </p>
          <ol class="guide" data-test="split-guide">
            <li class="done">
              <span class="mark" />
              <div class="col">
                <strong>This board is connected</strong>
                <span class="muted small">{{ nameOf(session) }}, firmware {{ info.firmware.join('.') }}</span>
                <div v-if="!canSplit(session)" class="note warn col" data-test="this-old">
                  <span>Its firmware is from before halves could share the work.</span>
                  <div v-if="desktop && isUsb" class="row wrap">
                    <button class="btn small primary" :disabled="flashing !== 'idle'" data-test="update-this" @click="updateThis">
                      {{ flashing === 'this' ? 'Updating… it comes back by itself' : 'Update this board' }}
                    </button>
                  </div>
                  <span v-else class="small">Update it under Hardware first.</span>
                </div>
              </div>
            </li>

            <li :class="other ? 'done' : 'now'" data-test="guide-other">
              <span class="mark" />
              <div class="col">
                <template v-if="other">
                  <strong>The other half is connected</strong>
                  <span class="muted small">{{ nameOf(other) }}, firmware {{ other.info.firmware.join('.') }}</span>
                  <label v-if="others.length > 1 && !isSplit" class="field small">
                    Several keyboards are open. Which one is the other half?
                    <select v-model="otherId" class="input" data-test="cable-other">
                      <option v-for="s in others" :key="s.id" :value="s.id">{{ nameOf(s) }}</option>
                    </select>
                  </label>
                  <div v-if="!canSplit(other)" class="note warn col" data-test="other-old">
                    <span>Its firmware is from before halves could share the work.</span>
                    <div v-if="desktop && isUsb" class="row wrap">
                      <button class="btn small primary" :disabled="flashing !== 'idle'" data-test="update-other" @click="updateOther">
                        {{ flashing === 'other' ? 'Updating… it comes back by itself' : 'Update the other half' }}
                      </button>
                    </div>
                    <span v-else class="small">Update it under its Hardware tab first.</span>
                  </div>
                </template>
                <template v-else>
                  <strong>Plug in the other half too</strong>
                  <span class="muted small">
                    It appears here by itself; there is nothing to click.
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
                      Is it a new board without JMK? Hold its BOOT (or BOOTSEL) button while plugging in its USB
                      cable, then let go. It is offered here for installing. Use a USB cable that carries data;
                      many carry power only. Boards with a single button (the 0xCB Helios, for one) work
                      differently: plug the board in, hold the button for a full second, let go.
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

            <li :class="isSplit ? 'done' : other ? 'now' : ''" data-test="guide-cable">
              <span class="mark" />
              <div class="col">
                <strong>{{ isSplit ? 'The cable between the halves is found' : 'Find the cable between the halves' }}</strong>
                <div v-if="cable.state === 'searching'" class="col" data-test="cable-searching">
                  <span class="muted small">Looking at every pin… neither half types for a few seconds.</span>
                  <progress :value="cable.progress" max="1" />
                </div>
                <span v-else-if="isSplit && cableOf(session)" class="muted small" data-test="cable-found">
                  This half sends on {{ pinLabel(board, cableOf(session)!.tx) }} and receives on
                  {{ pinLabel(board, cableOf(session)!.rx) }}. The other half is set to match.
                </span>
                <template v-else>
                  <span v-if="!other" class="muted small" data-test="cable-need-both">
                    Done by itself once both halves are here. You do not need to know which pins the cable is on.
                  </span>
                  <span v-else-if="!bothReady" class="muted small">Needs current firmware on both halves; see above.</span>
                  <div v-else class="row wrap">
                    <button class="btn small primary" data-test="find-cable" @click="findCable()">
                      Find the cable to {{ nameOf(other) }}
                    </button>
                    <span class="muted small">That keyboard cannot type for a few seconds while this runs.</span>
                  </div>
                  <p v-if="cable.state === 'failed'" class="note warn small" data-test="cable-problem">{{ cable.problem }}</p>
                </template>
              </div>
            </li>
          </ol>
          <div v-if="cable.state !== 'searching'" class="row wrap">
            <button v-if="isSplit && other && isFresh(other)" class="btn small ghost" data-test="find-cable-again" @click="findCable()">Look for the cable again</button>
            <button v-if="isSplit || session.hardware.split !== 'None'" class="btn small ghost" data-test="set-up-alone" @click="setUpAlone">
              Set this board up on its own instead
            </button>
          </div>
        </div>
        <BoardDiagram :board="board" :roles="roles" />
      </section>

      <!-- The cable between two halves, found without asking -------------------->
      <section v-else-if="view === 'cable'" key="cable" class="setup-fill" data-test="split-searching">
        <div class="setup-stage">
          <Transition name="words" mode="out-in">
            <div :key="String(cable.state === 'found')" class="words">
              <h1>{{ cable.state === 'found' ? 'Found the cable.' : 'Finding the cable between the halves.' }}</h1>
              <p class="muted lead">
                <template v-if="cable.state === 'found' && cableOf(session)">
                  It is on {{ pinLabel(board, cableOf(session)!.tx) }} and {{ pinLabel(board, cableOf(session)!.rx) }}. You never need to know that.
                </template>
                <template v-else>Looking for the serial pins it is wired to. Nothing to do.</template>
              </p>
            </div>
          </Transition>
          <SplitPicture
            :boards="[{ label: 'first board', state: 'ready' }, { label: 'second board', state: 'ready' }]"
            :cable="cable.state === 'found' ? 'found' : 'searching'"
            :progress="cable.progress"
          />
          <div class="below" />
        </div>
      </section>

      <!-- Keys --------------------------------------------------------------->
      <section v-else-if="view === 'keys'" key="keys" class="page centre">
        <h1 data-test="keys-heading">{{ intended && !firstId ? 'Press any key under your left hand.' : 'Press every key once.' }}</h1>
        <p v-if="!keyTotal" class="muted lead">
          <template v-if="intended">That tells the app which half is the left one. Then every key once, in any order.</template>
          <template v-else>Any order. The app works out the wiring from it.</template>
        </p>
        <span v-if="intended && !keyTotal" class="muted small" data-test="experimental">Split keyboards are experimental.</span>

        <div class="tally row">
          <strong class="count" data-test="count">
            <Transition name="tick" mode="out-in"><span :key="keyTotal">{{ keyTotal }}</span></Transition>
            {{ keyTotal === 1 ? 'key' : 'keys' }}
          </strong>
          <span v-if="!keyTotal" class="waiting">Waiting for the first key…</span>
        </div>

        <Transition name="rise">
          <div v-if="picture && !isSplit" class="col arrival" data-test="arrival">
            <SplitPicture :boards="picture.boards" :cable="picture.cable" :progress="cable.progress" />
            <div v-if="arrivals.length" class="row wrap centre-row">
              <button class="btn small primary" :disabled="flashing !== 'idle'" data-test="install-other" @click="installWaiting">
                {{ flashing === 'new' ? 'Installing the firmware…' : 'Install the firmware on the new board' }}
              </button>
              <span class="muted small">Afterwards it joins in here by itself.</span>
            </div>
            <p v-if="flashProblem" class="note warn small">{{ flashProblem }}</p>
          </div>
        </Transition>
        <Transition name="rise">
          <div v-if="lonelyHalf" class="note warn col" data-test="lonely-half">
            <span v-if="awayKeys !== undefined">
              <strong>The other half was unplugged.</strong> Plug it back in to carry on.
              <template v-if="awayKeys">Its {{ awayKeys }} {{ awayKeys === 1 ? 'key is' : 'keys are' }} kept.</template>
            </span>
            <span v-else>
              <strong>This is one half of a split keyboard.</strong> To set its keys up again, plug the other
              half into the computer too. It joins in here by itself.
            </span>
            <div class="row centre-row"><button class="btn small ghost" data-test="set-up-alone" @click="setUpAlone">Set this board up on its own instead</button></div>
          </div>
        </Transition>
        <Transition name="rise">
          <div v-if="cable.state === 'failed' && !isSplit" class="note warn" data-test="cable-problem">
            {{ cable.problem }} <button class="link" @click="go('shape')">Look again</button>, or carry on to set
            this board up on its own.
          </div>
        </Transition>

        <div class="halves" :class="{ single: !isSplit }">
          <div v-for="(half, index) in ordered" :key="half.id" class="half col" :class="{ live: isSplit && shown === half && keyTotal > 0 }" :data-test="'found-' + (index + 1)">
            <div v-if="isSplit" class="row half-name">
              <span class="chip" :class="{ accent: shown === half && keyTotal > 0 }">{{ firstId ? sideName(half) : index ? 'the other half' : 'one half' }}</span>
              <span class="muted small">
                <template v-if="setupOf(half).collector.keys.length">{{ setupOf(half).collector.keys.length }} keys</template>
                <template v-else>no key pressed on this half yet</template>
              </span>
            </div>
            <div class="found" :data-test="half === session ? 'found' : 'found-other'">
              <TransitionGroup name="pop">
                <div
                  v-for="(key, n) in setupOf(half).collector.keys"
                  :key="describeKey(half, key)"
                  class="found-key"
                  :class="{ last: shown === half && setupOf(half).collector.last === n }"
                  :title="describeKey(half, key)"
                >
                  <span :key="shown === half && setupOf(half).collector.last === n ? bump : 0" class="cap">{{ n + 1 }}</span>
                  <button class="remove" title="Not a key" @click="setupOf(half).collector.remove(n)">×</button>
                </div>
              </TransitionGroup>
            </div>
          </div>
        </div>

        <Transition name="rise">
          <div v-if="halves.some((half) => setupOf(half).collector.confused)" class="note warn">
            That looked like more than one key at the same instant. Press them one after the other.
          </div>
        </Transition>
        <Transition name="rise">
          <div v-if="stuckPins" class="note warn">
            Some pins were already connected before you pressed anything, so they are ignored. If that is not a
            stuck key, <button class="link" @click="go('board')">mark them as protected</button>.
          </div>
        </Transition>
        <div class="knobs col">
          <template v-for="half in ordered" :key="half.id">
            <div v-for="(encoder, index) in setupOf(half).encoders" :key="encoder.a" class="row centre-row">
              <span class="chip">knob {{ index + 1 }}<template v-if="isSplit"> · {{ sideName(half).toLowerCase() }}</template></span>
              <span class="mono">{{ pinLabel(boardOf(half), encoder.a) }} + {{ pinLabel(boardOf(half), encoder.b) }}</span>
              <button class="btn small ghost" @click="setupOf(half).encoders.splice(index, 1)">remove</button>
            </div>
          </template>
          <div v-if="findingKnob" class="note">
            <strong>Turn the knob a few clicks back and forth…</strong>
            <button class="btn small ghost" @click="cancelKnob">cancel</button>
          </div>
        </div>

        <div class="under row" :class="{ single: !isSplit }">
          <span v-if="unequal" class="muted small hint" data-test="unequal">
            Left {{ unequal.left }} · right {{ unequal.right }}. Halves usually match: {{ unequal.more }} to go on the {{ unequal.on }}?
          </span>
          <span class="grow" />
          <!-- A nod to how this works: the pins light up as keys are pressed. -->
          <button class="mcu row" data-test="show-pins" @click="pins = true">
            <BoardThumb :board="boardOf(shown)" :active="[...setupOf(shown).collector.active]" :taken="takenPins" />
            <span class="col">
              <strong>{{ boardName }}</strong>
              <span class="muted small">
                <template v-if="setupOf(shown).collector.last !== undefined">
                  {{ describeKey(shown, setupOf(shown).collector.keys[setupOf(shown).collector.last!]) }}
                </template>
                <template v-else>its pins light up as you press</template>
              </span>
              <span class="more small">show the pins ›</span>
            </span>
          </button>
        </div>
      </section>

      <!-- Only when the presses did not add up to a wiring ------------------->
      <section v-else key="result" class="page two">
        <div class="col text">
          <span class="chip accent">Not understood yet</span>
          <h1>Something is unusual.</h1>
          <div class="note warn">{{ problem }}</div>
          <p class="muted">Go back to press the keys again, or fix the wiring and retry.</p>
        </div>
        <div class="col visuals">
          <BoardDiagram :board="board" :roles="roles" />
        </div>
      </section>

    </Transition>

    <footer class="setup-bar row">
      <template v-if="view === 'keys'">
        <button class="btn ghost" :disabled="!keyTotal" data-test="undo-key" @click="undoLast">Undo</button>
        <button v-if="isSplit" class="btn ghost" :disabled="!firstId" data-test="swap-sides" @click="swapSides">Swap sides</button>
        <div class="menu-wrap">
          <button class="btn ghost" :class="{ on: menu }" data-test="special" @click="menu = !menu">Something special ▾</button>
          <Transition name="rise">
            <div v-if="menu" class="menu card col" @click="menu = false">
              <button v-if="!findingKnob && setupOf(shown).encoders.length < LIMITS.encoders" class="aside" data-test="find-knob" @click="findKnob">
                <span class="plus">+</span> It has a rotary knob
              </button>
              <button class="aside" data-test="open-board" @click="go('board')">
                <span class="plus">›</span> Another controller, or pins to protect
              </button>
              <button class="aside" data-test="open-shape" @click="go('shape')">
                <span class="plus">›</span> {{ isSplit ? 'The two halves and their cable' : 'It is one half of a split keyboard' }}
              </button>
              <button class="aside" data-test="explain" @click="showPins(true)">
                <span class="plus">?</span> How pressing keys tells it the wiring
              </button>
            </div>
          </Transition>
        </div>
      </template>
      <button v-else-if="view !== 'cable'" class="btn ghost" data-test="back" @click="go('keys')">Back</button>
      <span v-else class="muted small">continues by itself</span>
      <span class="grow" />
      <button v-show="view !== 'cable'" class="btn primary" :disabled="!canContinue || applying || view === 'cable'" data-test="next" @click="next">
        {{ applying ? 'Saving…' : view === 'keys' || view === 'cable' ? "That's all of them" : view === 'result' ? 'Press the keys again' : isSplit ? 'Press the keys' : 'Continue' }}
      </button>
    </footer>
    </div>

    <!-- The whole controller, for whoever wants to see what the app sees. -->
    <Transition name="fade">
      <div v-if="pins" class="overlay" data-test="pins-overlay" @click.self="pins = false">
        <div class="sheet card col">
          <div class="row">
            <strong>{{ boardName }}</strong>
            <span v-if="isSplit && firstId" class="chip accent">{{ sideName(shown) }}</span>
            <span class="grow" />
            <button class="btn small ghost" data-test="explain-toggle" @click="explaining = !explaining">{{ explaining ? 'Back to the board' : 'How does this work?' }}</button>
            <button class="btn small" data-test="close-pins" @click="pins = false">Close</button>
          </div>
          <div v-if="explaining" class="col">
            <MatrixExplainer />
            <p class="muted small">
              A controller has fewer pins than a keyboard has keys, which is why keys are wired in a grid like this.
            </p>
          </div>
          <BoardDiagram v-else :board="boardOf(shown)" :roles="roles" :active="setupOf(shown).collector.active" data-test="pins-diagram" />
          <p class="muted small">
            While you press keys, only weak test currents are used, and only on the yellow pins.
            <button class="link" data-test="not-this-board" @click="pins = false; go('board')">Not this board, or pins to protect?</button>
          </p>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.stage {
  flex: 1;
  min-width: 0;
  height: 100%;
}

.centre {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  gap: 12px;
}

.middle {
  justify-content: safe center;
}

.centre-row {
  justify-content: center;
}

.halves {
  display: flex;
  gap: 14px;
  align-self: stretch;
  width: 100%;
  max-width: 760px;
  margin: 0 auto;
}

.halves.single {
  max-width: 520px;
}

.half {
  flex: 1;
  min-width: 0;
  min-height: 112px;
  gap: 8px;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--panel);
  text-align: left;
  transition: border-color 0.2s var(--ease);
}

.half.live {
  border-color: var(--accent);
}

.under {
  align-self: stretch;
  width: 100%;
  max-width: 760px;
  margin: 0 auto;
  align-items: center;
}

.under.single {
  max-width: 520px;
}

.under .hint {
  text-align: left;
  max-width: 260px;
}

.mcu {
  gap: 10px;
  align-items: center;
  padding: 6px 12px 6px 8px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel);
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.mcu:hover {
  border-color: var(--muted);
}

.mcu .more {
  color: var(--accent);
}

.menu-wrap {
  position: relative;
}

.menu {
  position: absolute;
  bottom: calc(100% + 8px);
  left: 0;
  width: 320px;
  padding: 6px;
  gap: 2px;
  z-index: 5;
  box-shadow: var(--shadow);
}

.overlay {
  position: absolute;
  inset: 0;
  z-index: 20;
  display: grid;
  place-items: center;
  background: rgba(0, 0, 0, 0.5);
  padding: 20px;
}

.sheet {
  width: min(520px, 100%);
  max-height: 100%;
  overflow: auto;
  gap: 12px;
  padding: 16px;
  box-shadow: var(--shadow);
}

.big {
  max-width: 460px;
}
.arrival {
  gap: 8px;
  align-self: stretch;
}

.fixes {
  align-self: stretch;
  min-height: 30px;
}

.wizard {
  display: flex;
  height: 100%;
  position: relative;
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

.experimental {
  align-self: flex-start;
}

.half-keys {
  gap: 6px;
  align-self: stretch;
}

.half-name {
  gap: 10px;
}

.board-name {
  align-self: center;
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
