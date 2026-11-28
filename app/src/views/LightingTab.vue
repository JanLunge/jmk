<script setup lang="ts">
// RGB lights: where the chain is wired, where each light sits, what it shows.
import { computed, onBeforeUnmount, ref } from 'vue'
import KeyboardView from '../components/KeyboardView.vue'
import { partPins, wiringParts } from '../lib/definition'
import { attempt, toast } from '../lib/app'
import { findBoard, pinLabel, wirablePins } from '../lib/boards'
import { legend } from '../lib/keycodes'
import { bounds } from '../lib/layout'
import { drawnAt, fromWire, hexToHueSat, lightMode, placeAtKey, ring, shown, underKeys, zoneHex, zonePreview, type LightMode } from '../lib/lighting'
import type { Effect, Indicator, Indicators, Led, LightingConfig, LightZone, Pin, Shows } from '../lib/protocol'
import { LIMITS, PLAIN_LIGHT, UNDERGLOW } from '../lib/protocol'
import { linkPins, canIndicate, canLight, followsPins, knobPositions, needsRestart, restart, role, save, setIndicators, setLighting, sharesLights, type Session } from '../lib/session'

// `session` is the half whose lights are shown. For a split with both halves
// open, `halves` offers the choice and `main` is the half this editor is for.
const props = defineProps<{ session: Session; main?: Session; halves?: { id: string; side: string }[] }>()
const emit = defineEmits<{ half: [id: string] }>()
/** What the lights show is the same on both halves: it is written to both, so neither overrules the other at the next save. */
const sharing = computed(() => (props.main && props.main !== props.session ? [props.session, props.main] : [props.session]))
const show = (zones: Partial<Pick<LightingConfig, 'keys' | 'underglow'>>) =>
  attempt(async () => {
    for (const each of sharing.value) await setLighting(each, { ...each.lighting, ...zones })
    return true
  })

const lighting = computed(() => props.session.lighting)
const board = computed(() => findBoard(props.session.meta.controller, props.session.info.mcu, props.session.info.usable_pins))
/** One half of a split whose lights go with the other half's. */
const half = computed(() => role(props.session) !== 'single' && sharesLights(props.session))
const change = (patch: Partial<LightingConfig>) => attempt(() => setLighting(props.session, { ...lighting.value, ...patch }))

// --- The chain ---------------------------------------------------------------
/** Pins nothing else uses. */
const freePins = computed(() => {
  const { reserved_pins, encoders } = props.session.hardware
  const taken = new Set<Pin>([...reserved_pins, ...encoders.flatMap((e) => [e.a, e.b]), ...wiringParts(props.session.hardware).flatMap(partPins)])
  const link = linkPins(props.session.hardware)
  if (link) [link.tx, link.rx].forEach((pin) => taken.add(pin))
  if (indicators.value.pin !== null) taken.add(indicators.value.pin)
  return wirablePins(board.value, props.session.info.usable_pins).filter((pin) => !taken.has(pin))
})
const pinDraft = ref<Pin | null>(lighting.value.pin ?? null)
const countDraft = ref(lighting.value.leds.length || 12)

/** First setup: every light on in one colour, so a working chain is seen at once. */
async function turnOn() {
  if (pinDraft.value === null) return
  const count = Math.max(1, Math.min(LIMITS.leds, Math.round(countDraft.value)))
  const zone: LightZone = { effect: 'Rainbow', hue: 0, sat: 255, val: 255, speed: 60 }
  // The other half of a split may have said what the lights show already.
  const { keys, underglow } = lighting.value
  const zones = keys.effect !== 'Off' || underglow.effect !== 'Off' ? {} : { keys: { ...zone }, underglow: { ...zone } }
  const done = await attempt(async () => {
    await setLighting(props.session, { ...lighting.value, pin: pinDraft.value, leds: ring(props.session.layout, count) })
    return true
  })
  if (done && zones.keys) await show(zones)
  if (done) await saveAndRestart()
}

const restarting = ref(false)
async function saveAndRestart() {
  restarting.value = true
  await attempt(async () => {
    await save(props.session)
    if (needsRestart(props.session)) await restart(props.session)
    toast('Lights switched on')
  })
  restarting.value = false
}

function setCount(event: Event) {
  const count = Math.max(1, Math.min(LIMITS.leds, Math.round(Number((event.target as HTMLInputElement).value)) || 1))
  find(undefined)
  change({ leds: arranged(mode.value, count) })
}

// --- Underglow or per key ------------------------------------------------------
/** Chosen here; otherwise it follows from where the lights are. */
const chosenMode = ref<LightMode>()
const mode = computed(() => chosenMode.value ?? lightMode(lighting.value.leds))

/** `count` lights for a mode: a ring for underglow, one per key otherwise, keeping the ones already placed by hand. */
function arranged(to: LightMode, count: number): Led[] {
  if (to === 'underglow') return ring(props.session.layout, count)
  const kept = lighting.value.leds.slice(0, count)
  const fresh = underKeys(props.session.layout, count, knobPositions(props.session))
  if (to === 'keys') return fresh.map((led, index) => (kept[index] && kept[index].key !== UNDERGLOW ? kept[index] : led))
  return [...kept, ...ring(props.session.layout, count).slice(kept.length)]
}

async function setMode(to: LightMode) {
  chosenMode.value = to
  asUnderglow.value = to === 'underglow'
  await find(undefined)
  await change({ leds: arranged(to, lighting.value.leds.length) })
  // Lights under keys only help once each is matched to its key.
  if (to !== 'underglow') await find(0)
}

function placeByHand() {
  asUnderglow.value = mode.value === 'underglow'
  return find(0)
}

const removeLights = () => {
  find(undefined)
  return change({ pin: null, leds: [] })
}

// --- Where each light sits -----------------------------------------------------
/** The light being placed: it alone is lit on the keyboard. */
const finding = ref<number>()
const asUnderglow = ref(false)

/** Have the keyboard light whichever light is being placed now. */
let lit: number | null | undefined
let showing = false
async function showFinding() {
  if (showing) return
  showing = true
  // Clicks can come faster than the keyboard answers; catch up to the newest.
  while (lit !== (finding.value ?? null)) {
    const want = finding.value ?? null
    if (!(await attempt(() => props.session.client.identifyLed(want).then(() => true)))) break
    lit = want
  }
  showing = false
}

function find(index: number | undefined) {
  finding.value = index !== undefined && index < lighting.value.leds.length ? index : undefined
  return showFinding()
}

async function placeAt(pos: number) {
  const index = finding.value
  if (index === undefined) return
  const leds = lighting.value.leds.map((led, i) =>
    i === index ? { key: asUnderglow.value ? UNDERGLOW : pos, ...placeAtKey(props.session.layout, pos) } : led,
  )
  // Move on before talking to the keyboard, so a quick second click places the next light.
  finding.value = index + 1 < leds.length ? index + 1 : undefined
  if (finding.value === undefined) toast('All lights placed')
  await change({ leds })
  await showFinding()
}

onBeforeUnmount(() => {
  if (finding.value !== undefined) props.session.client.identifyLed(null).catch(() => {})
})

// --- The picture -----------------------------------------------------------------
const keyLights = computed(() => lighting.value.leds.filter((led) => led.key !== UNDERGLOW).length)
const underLights = computed(() => lighting.value.leds.length - keyLights.value)

const EDGE = 28
const drawn = computed(() => {
  const { leds, order, limit, keys, underglow } = lighting.value
  // A virtual keyboard's lights can be watched; a real one's are imagined.
  const live = props.session.lights.length === leds.length ? props.session.lights : undefined
  return leds.map((led, index) => {
    const under = led.key === UNDERGLOW
    const rgb = live ? fromWire(order, live[index]) : finding.value !== undefined ? (finding.value === index ? [90, 90, 90] : [0, 0, 0]) : zonePreview(under ? underglow : keys, limit)
    const at = drawnAt(props.session.layout, led)
    if (under) {
      // A light on the keyboard's edge is drawn just outside it, where its glow is seen.
      const box = bounds(props.session.layout)
      if (at.x <= box.x + 1) at.x -= EDGE
      else if (at.x >= box.x + box.w - 1) at.x += EDGE
      if (at.y <= box.y + 1) at.y -= EDGE
      else if (at.y >= box.y + box.h - 1) at.y += EDGE
    }
    // A key's light is drawn at its lower edge, clear of what is printed on the key.
    return { x: at.x, y: under ? at.y : at.y + 30, color: shown(rgb), under, marked: finding.value === index }
  })
})

const keyLegend = (pos: number) => {
  const action = props.session.layers[0]?.[pos]
  return action === undefined ? undefined : legend(action)
}

// --- What they show ----------------------------------------------------------------
const EFFECTS: { id: Effect; name: string; hint: string }[] = [
  { id: 'Off', name: 'Off', hint: 'Dark.' },
  { id: 'Solid', name: 'Colour', hint: 'One steady colour.' },
  { id: 'Breathing', name: 'Breathe', hint: 'One colour, fading in and out.' },
  { id: 'Rainbow', name: 'Rainbow', hint: 'All colours, moving across the keyboard.' },
  { id: 'Reactive', name: 'On press', hint: 'Dark until you type; the pressed key flashes and fades.' },
]
const zones = computed(() =>
  [
    { id: 'keys' as const, name: 'Under the keys', count: keyLights.value },
    { id: 'underglow' as const, name: 'Underglow', count: underLights.value },
  ].filter((zone) => zone.count > 0),
)
const MODES: { id: LightMode; name: string; hint: string }[] = [
  { id: 'underglow', name: 'Underglow', hint: 'Under the case, shining out around the edge. The lights are spread around the keyboard for you.' },
  { id: 'keys', name: 'Per key', hint: 'One light under each key. JMK lights them one at a time and you click the key each sits under.' },
  { id: 'both', name: 'Both', hint: 'Some under keys, some under the case. You say which is which while placing them.' },
]
const patchZone = (id: 'keys' | 'underglow', patch: Partial<LightZone>) => show({ [id]: { ...lighting.value[id], ...patch } })
const number = (event: Event) => Number((event.target as HTMLInputElement).value)

// --- Lights that show a state ---------------------------------------------------------
const indicators = computed(() => props.session.indicators)
const indicate = (patch: Partial<Indicators>) => attempt(() => setIndicators(props.session, { ...indicators.value, ...patch }))
/** For the plain light: the board's own first, then every pin nothing else uses. */
const plainPins = computed(() => {
  const own = board.value.internal.filter((part) => part.use.includes('LED')).map((part) => part.pin)
  const free = freePins.value.filter((pin) => pin !== lighting.value.pin)
  return [...new Set([...own, ...(indicators.value.pin === null ? [] : [indicators.value.pin]), ...free])].map((pin) => ({
    pin,
    name: own.includes(pin) ? `On the board (${pinLabel(board.value, pin)})` : pinLabel(board.value, pin),
  }))
})
const indicatorLights = computed(() => [
  ...(indicators.value.pin === null ? [] : [{ led: PLAIN_LIGHT, name: 'Plain light' }]),
  ...lighting.value.leds.map((_, index) => ({ led: index, name: `Light ${index + 1}` })),
])
const STATES = computed<{ shows: Shows; name: string }[]>(() => [
  { shows: 'CapsLock', name: 'Caps lock' },
  { shows: 'NumLock', name: 'Num lock' },
  { shows: 'ScrollLock', name: 'Scroll lock' },
  // Layer 0 is always on.
  ...props.session.layers.slice(1).map((_, index) => ({ shows: { Layer: index + 1 }, name: `Layer ${index + 1}` })),
])
function setPlainPin(event: Event) {
  const value = (event.target as HTMLSelectElement).value
  const pin = value === '' ? null : Number(value)
  return indicate({ pin, list: indicators.value.list.filter((rule) => pin !== null || rule.led !== PLAIN_LIGHT) })
}
const setIndicator = (index: number, patch: Partial<Indicator>) =>
  indicate({ list: indicators.value.list.map((rule, at) => (at === index ? { ...rule, ...patch } : rule)) })
const addIndicator = () => indicate({ list: [...indicators.value.list, { led: indicatorLights.value[0].led, shows: 'CapsLock', hue: 0, sat: 0 }] })

/** Rough current at full white, to explain the power limit: 60 mA per light. */
const peakMa = computed(() => Math.round((lighting.value.leds.length * 60 * lighting.value.limit) / 255))
/** Older firmware only looks at the pin when it starts. */
const pinChanged = computed(() => !followsPins(props.session) && lighting.value.pin !== props.session.bootLightPin)

/** The wiring is set once and then left alone: it stays folded away unless it is being worked on. */
const chainOpen = ref(false)
const showChain = computed(() => chainOpen.value || finding.value !== undefined)
const chainSummary = computed(() => {
  const count = lighting.value.leds.length
  const pin = lighting.value.pin === null ? '' : ` on ${pinLabel(board.value, lighting.value.pin)}`
  return `${count} ${count === 1 ? 'light' : 'lights'}${pin} · ${MODES.find((option) => option.id === mode.value)?.name.toLowerCase()}`
})
</script>

<template>
  <div class="lighting">
    <div class="stage col">
      <div class="board scroll">
        <KeyboardView
          :layout="session.layout"
          :legend="keyLegend"
          :pressed="session.pressed"
          :knobs="knobPositions(session)"
          :lights="drawn"
          :interactive="finding !== undefined"
          data-test="light-board"
          @down="placeAt"
        />
      </div>
      <p v-if="finding !== undefined" class="muted small hint">One light is lit on the keyboard now. Click the key it sits under, or nearest to.</p>
    </div>

    <aside class="side card col scroll">
      <div v-if="halves?.length" class="modes row" data-test="light-halves" title="Each half has its own lights. The half without a USB cable is set up through the cable between them.">
        <button v-for="each in halves" :key="each.id" class="mode" :class="{ on: each.id === session.id }" :data-test="'light-half-' + each.side" @click="emit('half', each.id)">
          {{ each.side === 'left' ? 'Left half' : 'Right half' }}
        </button>
      </div>
      <div v-if="!canLight(session)" class="note warn" data-test="light-old">
        This keyboard's firmware is from before JMK could run lights. Update it under Hardware first.
      </div>

      <template v-else-if="lighting.pin === null">
        <div>
          <h2>Lights</h2>
          <p class="muted small">For a chain of addressable RGB lights (WS2812, SK6812) on one data wire.</p>
          <p v-if="half && !halves?.length" class="muted small" data-test="light-half">These are the lights of the half that is plugged in. Plug in the other half to set up its own.</p>
        </div>
        <div class="fields">
          <label class="field">Data pin
            <select v-model="pinDraft" class="input" data-test="light-pin" title="The pin the chain's data wire is soldered to">
              <option :value="null" disabled>Choose</option>
              <option v-for="pin in freePins" :key="pin" :value="pin">{{ pinLabel(board, pin) }}</option>
            </select>
          </label>
          <label class="field">Lights
            <input v-model.number="countDraft" class="input" type="number" min="1" :max="LIMITS.leds" data-test="light-count" title="Unsure? Start too high: extra ones do no harm, and the count can be lowered later." />
          </label>
        </div>
        <button
          class="btn primary"
          :disabled="pinDraft === null || restarting"
          title="Brightness starts limited, so the lights cannot overload the USB port."
          data-test="light-on"
          @click="turnOn"
        >
          {{ restarting ? 'Restarting the keyboard…' : 'Switch the lights on' }}
        </button>
      </template>

      <template v-else>
        <p v-if="pinChanged" class="note warn" data-test="light-restart">
          The lights start working after a save and restart.
          <button class="btn small primary" :disabled="restarting" @click="saveAndRestart">Save and restart</button>
        </p>

        <!-- What they show: the part that is used day to day -->
        <section v-for="zone in zones" :key="zone.id" class="zone col" :data-test="'zone-' + zone.id">
          <header class="row">
            <span class="swatch" :style="{ background: lighting[zone.id].effect === 'Off' ? 'transparent' : lighting[zone.id].effect === 'Rainbow' ? 'conic-gradient(#f55, #fd5, #5d7, #5cf, #a6f, #f55)' : zoneHex(lighting[zone.id]) }" />
            <h3 class="grow">{{ zone.name }}</h3>
            <span class="muted small">{{ zone.count }}</span>
          </header>
          <div class="effects row" :data-test="'effect-' + zone.id">
            <button
              v-for="effect in EFFECTS"
              :key="effect.id"
              class="effect"
              :class="{ on: lighting[zone.id].effect === effect.id }"
              :title="effect.hint"
              :data-test="'effect-' + zone.id + '-' + effect.id"
              @click="patchZone(zone.id, { effect: effect.id })"
            >
              {{ effect.name }}
            </button>
          </div>
          <div v-if="lighting[zone.id].effect !== 'Off'" class="dials">
            <template v-if="lighting[zone.id].effect !== 'Rainbow'">
              <span class="label">Colour</span>
              <input type="color" :value="zoneHex(lighting[zone.id])" :data-test="'colour-' + zone.id" @input="patchZone(zone.id, hexToHueSat(($event.target as HTMLInputElement).value))" />
            </template>
            <span class="label">Brightness</span>
            <input type="range" min="0" max="255" :value="lighting[zone.id].val" :data-test="'brightness-' + zone.id" @input="patchZone(zone.id, { val: number($event) })" />
            <template v-if="lighting[zone.id].effect === 'Breathing' || lighting[zone.id].effect === 'Rainbow'">
              <span class="label">Speed</span>
              <input type="range" min="0" max="255" :value="lighting[zone.id].speed" @input="patchZone(zone.id, { speed: number($event) })" />
            </template>
          </div>
        </section>

        <p v-if="half" class="muted small" data-test="light-shared">Both halves show the same once this is saved.</p>

        <!-- The wiring: set once, then folded away -->
        <section class="chain col" :class="{ open: showChain }">
          <button class="chain-head row" data-test="chain-toggle" @click="chainOpen = !showChain">
            <span class="col grow">
              <strong>Setup</strong>
              <span class="muted small" data-test="chain-summary">{{ chainSummary }}</span>
            </span>
            <span class="muted">{{ showChain ? '▾' : '▸' }}</span>
          </button>
          <div v-if="showChain" class="col chain-body">
            <div class="modes row" data-test="light-mode">
              <button v-for="option in MODES" :key="option.id" class="mode" :class="{ on: mode === option.id }" :title="option.hint" :data-test="'mode-' + option.id" @click="setMode(option.id)">
                {{ option.name }}
              </button>
            </div>
            <button v-if="finding === undefined" class="btn small" :title="mode === 'underglow' ? 'For lights wired in another order than around the edge' : 'JMK lights them one at a time and you click where each sits'" data-test="place-lights" @click="placeByHand">
              Place the lights one by one
            </button>
            <div v-else class="note col" data-test="placing">
              <strong>Light {{ finding + 1 }} of {{ lighting.leds.length }} is lit</strong>
              <template v-if="mode === 'both'">
                <label class="row check"><input v-model="asUnderglow" type="radio" :value="false" data-test="place-key" /> Shines up through a key</label>
                <label class="row check"><input v-model="asUnderglow" type="radio" :value="true" data-test="place-under" /> Shines out of the bottom</label>
              </template>
              <span class="muted small">Click the key it sits under. None lit? The chain is shorter: lower the count.</span>
              <div class="row wrap">
                <button class="btn small" :disabled="finding === 0" @click="find(finding - 1)">Back</button>
                <button class="btn small" data-test="place-skip" @click="find(finding + 1)">Skip</button>
                <button class="btn small ghost" data-test="place-done" @click="find(undefined)">Done</button>
              </div>
            </div>
            <div class="fields">
              <label class="field">Lights
                <input class="input" type="number" min="1" :max="LIMITS.leds" :value="lighting.leds.length" data-test="light-count" @change="setCount" />
              </label>
              <label class="field">Data pin
                <select class="input" :value="lighting.pin" data-test="light-pin" @change="change({ pin: number($event) })">
                  <option :value="lighting.pin">{{ pinLabel(board, lighting.pin) }}</option>
                  <option v-for="pin in freePins.filter((p) => p !== lighting.pin)" :key="pin" :value="pin">{{ pinLabel(board, pin) }}</option>
                </select>
              </label>
            </div>
            <div class="dials" title="A USB port gives about 500 mA for the whole keyboard. Stay well under that.">
              <span class="label">Power limit</span>
              <input type="range" min="10" max="255" :value="lighting.limit" data-test="light-limit" @input="change({ limit: number($event) })" />
            </div>
            <span class="small" :class="peakMa > 450 ? 'over' : 'muted'" data-test="light-power">
              {{ Math.round((lighting.limit / 255) * 100) }}% · about {{ peakMa }} mA at full white<template v-if="peakMa > 450">: more than a USB port gives</template>
            </span>
            <label class="row check">
              <input type="checkbox" :checked="lighting.order === 'Rgb'" data-test="light-order" @change="change({ order: lighting.order === 'Rgb' ? 'Grb' : 'Rgb' })" />
              Red and green are swapped
            </label>
            <button class="btn small ghost remove" data-test="light-remove" @click="removeLights">Remove the lights</button>
          </div>
        </section>
      </template>

      <section v-if="canIndicate(session)" class="zone col" data-test="indicators">
        <header class="row"><h3 class="grow">Indicators</h3></header>
        <p class="muted small">A light can show caps lock or a layer instead of its effect.</p>
        <label class="field">Plain light
          <select class="input" :value="indicators.pin ?? ''" data-test="indicator-pin" title="A light of one colour on a pin of its own, like the one most controller boards carry" @change="setPlainPin">
            <option value="">None</option>
            <option v-for="option in plainPins" :key="option.pin" :value="option.pin">{{ option.name }}</option>
          </select>
        </label>
        <label v-if="indicators.pin !== null" class="row check" title="Tick this if the light is on when it should be off">
          <input type="checkbox" :checked="indicators.active_low" data-test="indicator-low" @change="indicate({ active_low: !indicators.active_low })" />
          It lights when the pin is low
        </label>
        <div v-for="(rule, index) in indicators.list" :key="index" class="row rule" data-test="indicator">
          <select class="input grow" :value="JSON.stringify(rule.shows)" data-test="indicator-shows" @change="setIndicator(index, { shows: JSON.parse(($event.target as HTMLSelectElement).value) })">
            <option v-for="state in STATES" :key="state.name" :value="JSON.stringify(state.shows)">{{ state.name }}</option>
          </select>
          <select class="input grow" :value="rule.led" data-test="indicator-led" @change="setIndicator(index, { led: number($event) })">
            <option v-for="light in indicatorLights" :key="light.led" :value="light.led">{{ light.name }}</option>
          </select>
          <input v-if="rule.led !== PLAIN_LIGHT" type="color" :value="zoneHex(rule)" title="Its colour while this is on" @input="setIndicator(index, hexToHueSat(($event.target as HTMLInputElement).value))" />
          <button class="btn small ghost" title="Remove" @click="indicate({ list: indicators.list.filter((_, at) => at !== index) })">×</button>
        </div>
        <button class="btn small" :disabled="!indicatorLights.length || indicators.list.length >= LIMITS.indicators" :title="indicatorLights.length ? '' : 'Choose a plain light or set up a chain first'" data-test="indicator-add" @click="addIndicator">
          Add an indicator
        </button>
      </section>
    </aside>
  </div>
</template>

<style scoped>
.lighting {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 360px;
  gap: 18px;
  height: 100%;
  min-height: 0;
  padding: 18px 24px;
}

.stage {
  min-height: 0;
  gap: 14px;
}

.board {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 10px;
}

.board > svg {
  flex: none;
}

.hint {
  text-align: center;
  min-height: 1.4em;
}

.side {
  min-height: 0;
  padding: 14px;
  gap: 12px;
  align-self: start;
  max-height: 100%;
}

.small {
  font-size: 0.82rem;
}

.zone,
.chain {
  gap: 10px;
  padding: 12px;
  border: 1px solid var(--line);
  border-radius: 12px;
}

.zone header {
  gap: 9px;
  align-items: center;
}

.zone h3 {
  margin: 0;
  font-size: 0.95rem;
}

.rule {
  gap: 6px;
  align-items: center;
}

.rule .input {
  min-width: 0;
}

.swatch {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 1px solid var(--line);
}

.effects,
.modes {
  gap: 0;
  padding: 2px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel-2);
}

.effect,
.mode {
  flex: 1;
  padding: 6px 4px;
  border: none;
  border-radius: 8px;
  background: none;
  color: var(--muted);
  font-size: 0.85rem;
  white-space: nowrap;
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}

.effect.on,
.mode.on {
  background: var(--accent);
  color: var(--accent-ink);
  font-weight: 600;
}

/* A label and its control per line. */
.dials {
  display: grid;
  grid-template-columns: 84px minmax(0, 1fr);
  gap: 8px 10px;
  align-items: center;
}

.dials .label {
  color: var(--muted);
  font-size: 0.85rem;
}

.chain {
  padding: 0;
  gap: 0;
}

.chain-head {
  gap: 10px;
  padding: 10px 12px;
  border: none;
  background: none;
  color: inherit;
  text-align: left;
  align-items: center;
  cursor: pointer;
}

.chain-body {
  gap: 10px;
  padding: 0 12px 12px;
}

.over {
  color: var(--danger);
}

.remove {
  align-self: flex-start;
  color: var(--muted);
}

.fields {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.check {
  cursor: pointer;
  gap: 8px;
  font-size: 0.92rem;
}

.check input,
input[type='range'] {
  accent-color: var(--accent);
}

input[type='color'] {
  width: 44px;
  height: 30px;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  cursor: pointer;
}

@media (max-width: 980px) {
  .lighting {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(0, 1fr) minmax(0, 1fr);
  }
}
</style>
