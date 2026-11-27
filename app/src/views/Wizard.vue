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
import { boardsFor, chipLabel, findBoard, genericBoard, pinLabel, pinsOfAnyShape, shapeFromPins, wirablePins } from '../lib/boards'
import { desktop, type BootDrive } from '../lib/desktop'
import { localPositions, parseDefinition, positionCount as definitionPositions, type Definition } from '../lib/definition'
import { arrivals as allArrivals, bootDrives, installArrival, installOnBoard, updateDue, updateFirmware } from '../lib/firmware'
import SplitPicture, { type PictureBoard } from '../components/SplitPicture.vue'
import { chooseLink, findWires } from '../lib/cable'
import { KeyCollector, KnobFinder } from '../lib/collector'
import { acknowledge, muted } from '../lib/sound'
import { inferWiring, type KeySignature } from '../lib/infer'
import { bounds, hasKey, NO_KEY, UNIT } from '../lib/layout'
import { ALPHAS, letterActions, rowLayout, splitRows } from '../lib/starter'
import LetterRows from '../components/LetterRows.vue'
import type { Action, Contact, EncoderConfig, HardwareConfig, KeyGeometry, Pin } from '../lib/protocol'
import { LIMITS } from '../lib/protocol'
import { applyDefinition, canKeepLetterRows, canShareWire, linkFor, isConfigured, knobSpot, linkPins, splitOffset, needsRestart, restart, save, setHardware, setLayout, setMeta, SPLIT_PROTOCOL, starterLayout, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

// Pressing the keys is the setup. The controller and the split pages are
// side trips for boards that need them; 'result' only shows when the
// wiring could not be read.
const STEPS = ['board', 'shape', 'keys', 'rows', 'letters', 'result'] as const
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
const boardName = computed(() => (boardId.value === AUTO && !settledShape.value ? `${chipLabel(info.mcu)} controller` : board.value.name))
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
/** The app carries newer firmware than this board runs, and can install it here. */
const stale = (s: Session) => !canSplit(s) || updateDue(s)
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
      canShareWire(props.session) && canShareWire(candidate),
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
  // Another keyboard on the desk is only a likely other half while it is new
  // or known to belong to this one. A keyboard in use is a keyboard of its own.
  const candidate = other.value && !other.value.closed ? other.value : undefined
  const near = candidate && (isFresh(candidate) || partnerOf(props.session) === candidate) ? candidate : undefined
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
  if (step.value === 'rows') return onRowPress(half, contacts)
  const encoderPins = new Set(setup.encoders.flatMap((e) => [e.a, e.b]))
  const relevant = contacts.filter((c) => !('Ground' in c && encoderPins.has(c.Ground)))
  const before = setup.collector.last
  const count = setup.collector.keys.length
  const presses = setup.collector.presses
  setup.collector.update(relevant)
  if (setup.collector.presses > presses) acknowledge(setup.collector.keys.length > count ? 'new' : 'known')
  if (setup.collector.keys.length > count || setup.collector.last !== before) {
    firstId.value ??= half.id
    shownId.value = half.id
  }
  if (setup.collector.keys.length > count) mine.history.push(half.id)
  bump.value++
}

// --- Rows ----------------------------------------------------------------------
// The keys were pressed row by row. Pressing the last key of each row once
// more tells where the rows end, and the picture can be drawn from that.
/** Per half, the keys (by their place in press order) that end a row. */
const rowEnds = ref<Record<string, number[]>>({})
const endsOf = (half: Session) => rowEnds.value[half.id] ?? []
const rowsOf = (half: Session) => splitRows(setupOf(half).collector.keys.length, endsOf(half))
const rowsMarked = computed(() => halves.value.some((half) => endsOf(half).length > 0))
/** A key pressed here that was not pressed before. */
const strayKey = ref(false)
function toggleRowEnd(half: Session, n: number) {
  const ends = endsOf(half)
  rowEnds.value = { ...rowEnds.value, [half.id]: ends.includes(n) ? ends.filter((end) => end !== n) : [...ends, n] }
  shownId.value = half.id
}
function onRowPress(half: Session, contacts: Contact[]) {
  const collector = setupOf(half).collector
  const encoderPins = new Set(setupOf(half).encoders.flatMap((e) => [e.a, e.b]))
  const [count, presses] = [collector.keys.length, collector.presses]
  collector.update(contacts.filter((c) => !('Ground' in c && encoderPins.has(c.Ground))))
  if (collector.keys.length > count) {
    // Not one of the keys found before: it has no place in the order yet.
    collector.remove(collector.keys.length - 1)
    strayKey.value = true
  } else if (collector.presses > presses && collector.last !== undefined) {
    strayKey.value = false
    toggleRowEnd(half, collector.last)
  }
  bump.value++
}
// Other keys, other rows.
watch(keyTotal, () => {
  rowEnds.value = {}
  strayKey.value = false
})

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

// The keyboard reports its contacts while keys are found and while the row ends are pressed.
const listens = (at: Step) => at === 'keys' || at === 'rows'
watch(step, async (now) => {
  if (listens(now) && !probing) await startProbing()
  else if (!listens(now) && now !== 'result') await stopProbing()
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
      more: result.wiring.more.length ? result.wiring.more : undefined,
      reserved_pins: [...reserved],
      debounce_ms: 5,
      encoders: [...setupOf(half).encoders],
      split: isSplit.value && link ? { Half: { link: linkFor(link), offset } } : 'None',
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
    for (const part of [result.value.wiring.matrix, ...result.value.wiring.more]) {
      if (part === 'None') continue
      if ('Direct' in part) part.Direct.pins.forEach((pin) => map.set(pin, 'direct'))
      else {
        part.Matrix.rows.forEach((pin) => map.set(pin, 'row'))
        part.Matrix.cols.forEach((pin) => map.set(pin, 'col'))
      }
    }
  }
  return map
})

/**
 * One picture and one keymap for the whole keyboard: the first half's keys,
 * then the second half's to the right of them.
 */
const drafted = computed(() => {
  const all = plans.value
  if (all.some((plan) => !plan)) return undefined
  const layout: KeyGeometry[] = []
  const actions: Action[] = []
  /** Positions that are a switch, not a knob. */
  const keys: number[] = []
  for (const [index, plan] of all.entries()) {
    const { half, hardware, positions, offset } = plan!
    const shift = layout.length ? Math.round(bounds(layout.filter(hasKey)).w + 2 * UNIT) : 0
    let own: KeyGeometry[]
    if (endsOf(half).length) {
      // Rows as they were pressed. The left half of a split keeps its short rows at the inner edge.
      const rows = rowsOf(half).map((row) => row.map((n) => positions[n]))
      own = rowLayout(rows, localPositions(hardware) - 2 * hardware.encoders.length, isSplit.value && index === 0 ? 'right' : 'left')
      hardware.encoders.forEach(() => own.push(knobSpot(own), { ...NO_KEY }))
    } else {
      own = starterLayout(hardware, positions)
    }
    layout.push(...own.map((key) => (hasKey(key) ? { ...key, x: key.x + shift } : key)))
    // Keys start empty; the letters step, or the keymap editor later, fills them.
    const mine: Action[] = Array(own.length).fill('Transparent')
    for (const pos of positions) mine[pos] = { Basic: 'None' }
    const firstKnob = own.length - 2 * hardware.encoders.length
    for (const knob of hardware.encoders.keys()) {
      mine[firstKnob + 2 * knob] = { Basic: { Consumer: 0xea } }
      mine[firstKnob + 2 * knob + 1] = { Basic: { Consumer: 0xe9 } }
    }
    while (actions.length < offset) actions.push('Transparent')
    keys.push(...positions.map((pos) => offset + pos))
    actions.push(...mine)
  }
  return { layout, actions, keys }
})

// --- Letters -------------------------------------------------------------------
// The user shows where the three letter rows are; the letters of a known
// arrangement go there, and the other keys stay empty for them to fill.
const alphasId = ref(ALPHAS[0].id)
const alphas = computed(() => ALPHAS.find((a) => a.id === alphasId.value) ?? ALPHAS[0])
/** The key positions of the top, home and bottom letter row. */
const letterRows = ref<number[][]>([[], [], []])
const lettersPlaced = computed(() => letterRows.value.some((row) => row.length > 0))
watch(rowEnds, () => (letterRows.value = [[], [], []]))
/**
 * Placing letter rows only makes sense on a keyboard that can hold the 26
 * letters. A macro pad goes from its rows straight to the editor.
 */
const fitsLetters = computed(() => keyTotal.value >= 26)
const letters = computed(() => letterActions(letterRows.value, alphas.value))
/** The keymap the keyboard starts with. */
const startActions = computed(() => {
  const draft = drafted.value
  if (!draft) return []
  const actions = [...draft.actions]
  for (const pos of draft.keys) actions[pos] = letters.value.get(pos) ?? { Basic: 'None' }
  return actions
})

const applying = ref(false)
async function apply() {
  const all = plans.value
  const draft = drafted.value
  if (!draft) return
  applying.value = true
  const actions = startActions.value
  const done = await attempt(async () => {
    await stopProbing()
    for (const plan of all) {
      const { half, hardware } = plan!
      await setHardware(half, hardware)
      await setLayout(half, draft.layout)
      await half.client.setActions(0, 0, actions)
      // Kept on the keyboard, so another arrangement can be put on the same keys later.
      if (canKeepLetterRows(half)) await half.client.setLetterRows(letterRows.value)
      half.letterRows = letterRows.value
      half.layers[0].splice(0, actions.length, ...actions)
      await setMeta(half, { name: name.value.trim().slice(0, LIMITS.nameLength), controller: board.value.id })
    }
    return true
  })
  // Saved and restarted, the keyboard opens in the editor: that is where it is tried and named.
  if (done) await finish()
  applying.value = false
}

/** Every key is pressed: with a wiring that makes sense, on to how the keys are arranged. */
function allPressed() {
  go(problem.value ? 'result' : 'rows')
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
  await leaveSetup(all)
}

/** The halves are set up: the editor takes over. */
async function leaveSetup(all: Session[]) {
  for (const half of all) {
    resume.delete(half.id)
    led.delete(half.id)
    kept.delete(half.id)
    half.setup = false
  }
  relink()
  app.currentId = all[0].id
  // Older firmware only starts using the cable with its next start.
  for (const half of all) {
    if (needsRestart(half)) await attempt(() => restart(half))
  }
}

// --- Restore from a file ---------------------------------------------------------
// A keyboard file saved before (Share → Save as file) puts wiring, layout and
// keymap back at once, for a board that was wiped or flashed anew.
const backup = ref<Definition>()
const backupInput = ref<HTMLInputElement>()
async function pickBackup(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  try {
    const chosen = parseDefinition(await file.text())
    if (chosen.mcu !== info.mcu) throw new Error(`This file is for a ${chipLabel(chosen.mcu)} controller.`)
    if (chosen.halves.length < halves.value.length) throw new Error('This file is for a keyboard in one piece, but two halves are being set up.')
    backup.value = chosen
  } catch (error) {
    toast(error instanceof Error ? error.message : String(error), 'error')
  }
}
/** The file's halves, first half first, the way the halves here are ordered. */
const backupHalves = computed(() =>
  backup.value ? [...backup.value.halves.keys()].sort((a, b) => splitOffset(backup.value!.halves[a].hardware) - splitOffset(backup.value!.halves[b].hardware)) : [],
)
async function restoreBackup() {
  const chosen = backup.value
  if (!chosen) return
  backup.value = undefined
  applying.value = true
  await stopProbing()
  const all = [...ordered.value]
  const done = await attempt(async () => {
    for (const [index, half] of all.entries()) await applyDefinition(half, chosen, backupHalves.value[index] ?? index)
    return true
  })
  applying.value = false
  if (!done) {
    if (step.value === 'keys') await startProbing()
    return
  }
  toast(`Restored ${chosen.name}`)
  await leaveSetup(all)
}

// --- What is on the screen -----------------------------------------------------
const view = computed(() => (step.value === 'keys' && (cable.state === 'searching' || found.value || aboutToSearch.value) ? 'cable' : step.value))
/** Two boards are in play, so finding their cable is a step of its own. */
const splitFlow = computed(
  () => intended.value || view.value === 'cable' || arrivals.value.length > 0 || (!alone.value && !!other.value && !other.value.closed && isFresh(other.value)),
)
const guideSteps = computed(() => [...(splitFlow.value ? ['Plug in', 'Firmware', 'Find the cable'] : ['Plug in', 'Firmware']), 'Find the keys', 'Rows', ...(fitsLetters.value ? ['Letters'] : [])])
const guideAt = computed(() => {
  if (view.value === 'cable') return 2
  const keysAt = guideSteps.value.indexOf('Find the keys')
  return keysAt + (view.value === 'rows' ? 1 : view.value === 'letters' ? 2 : 0)
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
  else if (step.value === 'rows') fitsLetters.value ? go('letters') : apply()
  else if (step.value === 'letters') apply()
  else go('keys')
}
const back = () => go(step.value === 'letters' ? 'rows' : 'keys')
const nextLabel = computed(() => {
  if (applying.value) return 'Saving…'
  if (view.value === 'keys' || view.value === 'cable') return "That's all of them"
  if (view.value === 'rows') return rowsMarked.value ? (fitsLetters.value ? 'Continue' : 'Finish') : 'Skip'
  if (view.value === 'letters') return lettersPlaced.value ? 'Finish' : 'Skip'
  if (view.value === 'result') return 'Press the keys again'
  return isSplit.value ? 'Press the keys' : 'Continue'
})
</script>

<template>
  <div class="wizard">
    <SetupGuide :steps="guideSteps" :current="guideAt" />
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
                <div v-if="stale(session)" class="note col" :class="{ warn: !canSplit(session) }" data-test="this-old">
                  <span v-if="!canSplit(session)">Its firmware is from before halves could share the work.</span>
                  <span v-else>This app carries newer firmware. A cable with a single data wire needs it.</span>
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
                  <div v-if="stale(other)" class="note col" :class="{ warn: !canSplit(other) }" data-test="other-old">
                    <span v-if="!canSplit(other)">Its firmware is from before halves could share the work.</span>
                    <span v-else>This app carries newer firmware. A cable with a single data wire needs it.</span>
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
                    It appears here by itself; there is nothing to click. Join the halves with their cable
                    before either has power: plugging that cable in shorts its contacts on the way.
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
                  <template v-if="cableOf(session)!.tx === cableOf(session)!.rx">
                    The cable has one data wire, on {{ pinLabel(board, cableOf(session)!.tx) }}. The halves take turns on it.
                  </template>
                  <template v-else>
                    This half sends on {{ pinLabel(board, cableOf(session)!.tx) }} and receives on
                    {{ pinLabel(board, cableOf(session)!.rx) }}. The other half is set to match.
                  </template>
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
                  <template v-if="cableOf(session)!.tx === cableOf(session)!.rx">
                    It has one data wire, on {{ pinLabel(board, cableOf(session)!.tx) }}. The halves take turns on it.
                  </template>
                  <template v-else>
                    It is on {{ pinLabel(board, cableOf(session)!.tx) }} and {{ pinLabel(board, cableOf(session)!.rx) }}. You never need to know that.
                  </template>
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
            <div v-if="desktop && isUsb && (stale(session) || (other && stale(other)))" class="row wrap centre-row">
              <button v-if="stale(session)" class="btn small primary" :disabled="flashing !== 'idle'" data-test="update-this-here" @click="updateThis">
                {{ flashing === 'this' ? 'Updating… it comes back by itself' : 'Update this board' }}
              </button>
              <button v-if="other && stale(other)" class="btn small primary" :disabled="flashing !== 'idle'" data-test="update-other-here" @click="updateOther">
                {{ flashing === 'other' ? 'Updating… it comes back by itself' : 'Update the other board' }}
              </button>
            </div>
            <p v-if="flashProblem" class="small">{{ flashProblem }}</p>
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

      <!-- Rows: where each one ends ------------------------------------------>
      <section v-else-if="view === 'rows'" key="rows" class="page centre">
        <h1>Press the last key of each row.</h1>
        <p class="muted lead">
          You pressed the keys row by row. The key at the right end of each row tells the app where the next row
          starts, so the picture comes out like your keyboard. Press one again to take it back.
        </p>
        <div class="halves" :class="{ single: !isSplit }">
          <div v-for="(half, index) in ordered" :key="half.id" class="half col" :class="{ live: isSplit && shown === half }">
            <div v-if="isSplit" class="row half-name">
              <span class="chip" :class="{ accent: shown === half }">{{ sideName(half) }}</span>
              <span class="muted small">{{ rowsOf(half).length }} {{ rowsOf(half).length === 1 ? 'row' : 'rows' }}</span>
            </div>
            <div class="lines col" :class="{ inner: isSplit && index === 0 }" :data-test="'rows-' + (index + 1)">
              <div v-for="(line, at) in rowsOf(half)" :key="at" class="line row">
                <button
                  v-for="n in line"
                  :key="n"
                  class="row-key"
                  :class="{ end: endsOf(half).includes(n), last: shown === half && setupOf(half).collector.last === n }"
                  :title="endsOf(half).includes(n) ? 'Ends its row. Click to take that back.' : 'Click if this is the last key of its row'"
                  :data-test="'row-key-' + (index + 1) + '-' + n"
                  @click="toggleRowEnd(half, n)"
                >
                  {{ n + 1 }}
                </button>
              </div>
            </div>
          </div>
        </div>
        <Transition name="rise">
          <div v-if="strayKey" class="note warn" data-test="stray-key">
            That key was not pressed before. <button class="link" @click="go('keys')">Go back</button> to add it.
          </div>
        </Transition>
      </section>

      <!-- Letters: where the three letter rows are --------------------------->
      <section v-else-if="view === 'letters'" key="letters" class="page centre">
        <h1>Where do the letters go?</h1>
        <p class="muted lead">Show where the three rows of letters are, and pick the arrangement that goes on them.</p>
        <LetterRows v-if="drafted" v-model:rows="letterRows" v-model:alphas="alphasId" :layout="drafted.layout" :keys="drafted.keys" />
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

    <Transition name="fade">
      <div v-if="backup" class="overlay" @click.self="backup = undefined">
        <div class="card col dialog" data-test="restore-dialog">
          <h2>Restore “{{ backup.name }}”?</h2>
          <p class="muted">
            {{ definitionPositions(backup.halves) }} key positions{{ backup.halves.length > 1 ? ' across two halves' : '' }},
            {{ backup.keymap.layers.length }} layer{{ backup.keymap.layers.length === 1 ? '' : 's' }}. The wiring, layout and keymap
            come from the file, so the keys need not be pressed.
          </p>
          <p v-if="backup.halves.length > halves.length" class="note warn">Only one half is here. Connect the other half first, or restore it on its own later.</p>
          <div class="row">
            <span class="grow" />
            <button class="btn ghost" @click="backup = undefined">Cancel</button>
            <button class="btn primary" data-test="restore-apply" @click="restoreBackup">Restore</button>
          </div>
        </div>
      </div>
    </Transition>

    <footer class="setup-bar row">
      <template v-if="view === 'keys'">
        <button class="btn ghost" :disabled="!keyTotal" data-test="undo-key" @click="undoLast">Undo</button>
        <button v-if="isSplit" class="btn ghost" :disabled="!firstId" data-test="swap-sides" @click="swapSides">Swap sides</button>
        <div class="menu-wrap">
          <input ref="backupInput" type="file" accept=".json,application/json" hidden data-test="restore-file" @change="pickBackup" />
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
              <button class="aside" data-test="restore" @click="backupInput?.click()">
                <span class="plus">↺</span> Restore from a keyboard file
              </button>
              <button class="aside" data-test="explain" @click="showPins(true)">
                <span class="plus">?</span> How pressing keys tells it the wiring
              </button>
            </div>
          </Transition>
        </div>
        <button
          class="btn ghost sound"
          :class="{ off: muted }"
          :title="muted ? 'Sound is off. Turn on the tone for each key press.' : 'Turn off the tone for each key press'"
          :aria-label="muted ? 'Turn on the sound for key presses' : 'Turn off the sound for key presses'"
          :aria-pressed="!muted"
          data-test="sound"
          @click="muted = !muted"
        >
          <svg viewBox="0 0 20 20" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M3.5 7.8h2.6L10 4.5v11l-3.9-3.3H3.5z" />
            <template v-if="muted"><path d="M13.2 8l3.6 4M16.8 8l-3.6 4" /></template>
            <template v-else><path d="M13 7.6a3.4 3.4 0 0 1 0 4.8M15.2 5.6a6.2 6.2 0 0 1 0 8.8" /></template>
          </svg>
        </button>
      </template>
      <button v-else-if="view !== 'cable'" class="btn ghost" data-test="back" @click="back">Back</button>
      <span v-else class="muted small">continues by itself</span>
      <span class="grow" />
      <button v-show="view !== 'cable'" class="btn primary" :disabled="!canContinue || applying || view === 'cable'" data-test="next" @click="next">
        {{ nextLabel }}
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
  min-height: 0;
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
  /* The picture is narrower than the page. */
  align-items: center;
}

.fixes {
  align-self: stretch;
  min-height: 30px;
}

.wizard {
  display: flex;
  flex-direction: column;
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

.sound {
  padding: 9px 10px;
}

.sound.off {
  color: var(--muted);
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

.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  display: grid;
  place-items: center;
  z-index: 40;
}

.dialog {
  width: min(480px, 90vw);
  box-shadow: var(--shadow);
}

/* Rows and letters */
.lines {
  gap: 6px;
  align-self: stretch;
}

.lines.inner {
  align-items: flex-end;
}

.line {
  gap: 6px;
  flex-wrap: wrap;
}

.row-key {
  position: relative;
  width: 38px;
  height: 38px;
  border: none;
  border-radius: 9px;
  background: var(--cap-top);
  border-bottom: 4px solid var(--cap);
  color: var(--text);
  font-weight: 700;
  font-size: 0.85rem;
  cursor: pointer;
  transition:
    background 0.15s,
    box-shadow 0.15s;
}

.row-key:hover {
  box-shadow: 0 0 0 2px var(--muted);
}

.row-key.end {
  background: var(--accent);
  color: var(--accent-ink);
}

.row-key.end::after {
  content: '↵';
  position: absolute;
  right: -5px;
  top: -7px;
  width: 17px;
  height: 17px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  background: var(--panel);
  border: 1px solid var(--accent);
  color: var(--accent);
  font-size: 0.65rem;
}

</style>
