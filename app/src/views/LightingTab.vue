<script setup lang="ts">
// RGB lights: where the chain is wired, where each light sits, what it shows.
import { computed, onBeforeUnmount, ref } from 'vue'
import KeyboardView from '../components/KeyboardView.vue'
import { attempt, toast } from '../lib/app'
import { findBoard, pinLabel, wirablePins } from '../lib/boards'
import { legend } from '../lib/keycodes'
import { bounds } from '../lib/layout'
import { drawnAt, fromWire, hexToHueSat, lightMode, placeAtKey, ring, shown, underKeys, zoneHex, zonePreview, type LightMode } from '../lib/lighting'
import type { Effect, Led, LightingConfig, LightZone, Pin } from '../lib/protocol'
import { LIMITS, UNDERGLOW } from '../lib/protocol'
import { canLight, knobPositions, needsRestart, restart, save, setLighting, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

const lighting = computed(() => props.session.lighting)
const board = computed(() => findBoard(props.session.meta.controller, props.session.info.mcu, props.session.info.usable_pins))
const change = (patch: Partial<LightingConfig>) => attempt(() => setLighting(props.session, { ...lighting.value, ...patch }))

// --- The chain ---------------------------------------------------------------
/** Pins nothing else uses. */
const freePins = computed(() => {
  const { matrix, reserved_pins, encoders, split } = props.session.hardware
  const taken = new Set<Pin>([...reserved_pins, ...encoders.flatMap((e) => [e.a, e.b])])
  if (matrix !== 'None' && 'Direct' in matrix) matrix.Direct.pins.forEach((pin) => taken.add(pin))
  if (matrix !== 'None' && 'Matrix' in matrix) [...matrix.Matrix.rows, ...matrix.Matrix.cols].forEach((pin) => taken.add(pin))
  const link = split === 'None' ? undefined : 'Central' in split ? split.Central : split.Peripheral
  if (link && link !== 'Ble') [link.Uart.tx, link.Uart.rx].forEach((pin) => taken.add(pin))
  return wirablePins(board.value, props.session.info.usable_pins).filter((pin) => !taken.has(pin))
})
const pinDraft = ref<Pin | null>(lighting.value.pin ?? null)
const countDraft = ref(lighting.value.leds.length || 12)

/** First setup: every light on in one colour, so a working chain is seen at once. */
async function turnOn() {
  if (pinDraft.value === null) return
  const count = Math.max(1, Math.min(LIMITS.leds, Math.round(countDraft.value)))
  const zone: LightZone = { effect: 'Rainbow', hue: 0, sat: 255, val: 255, speed: 60 }
  const done = await attempt(async () => {
    await setLighting(props.session, { ...lighting.value, pin: pinDraft.value, leds: ring(props.session.layout, count), keys: { ...zone }, underglow: { ...zone } })
    return true
  })
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
  { id: 'Off', name: 'Off', hint: '' },
  { id: 'Solid', name: 'One colour', hint: '' },
  { id: 'Breathing', name: 'Breathing', hint: 'Fades in and out.' },
  { id: 'Rainbow', name: 'Rainbow', hint: 'All colours, moving across the keyboard.' },
  { id: 'Reactive', name: 'On key press', hint: 'Dark until you type; the pressed key flashes and fades.' },
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
const patchZone = (id: 'keys' | 'underglow', patch: Partial<LightZone>) => change({ [id]: { ...lighting.value[id], ...patch } })
const number = (event: Event) => Number((event.target as HTMLInputElement).value)

/** Rough current at full white, to explain the power limit: 60 mA per light. */
const peakMa = computed(() => Math.round((lighting.value.leds.length * 60 * lighting.value.limit) / 255))
const pinChanged = computed(() => lighting.value.pin !== props.session.bootLightPin)
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
      <p class="muted small hint">
        <template v-if="finding !== undefined">
          One light is lit on the keyboard now. Click the key it sits under, or nearest to.
        </template>
        <template v-else-if="lighting.leds.length">
          Round dots are lights under keys; dashed ones shine out of the bottom. Changes show on the keyboard at once
          and stay after unplugging once you save.
        </template>
      </p>
    </div>

    <aside class="side card col scroll">
      <div v-if="!canLight(session)" class="note warn" data-test="light-old">
        This keyboard's firmware is from before JMK could run lights. Update it under Hardware first.
      </div>

      <template v-else-if="lighting.pin === null">
        <div>
          <h2>Lights</h2>
          <p class="muted small">
            For addressable RGB lights (WS2812, SK6812 and the like): a chain of lights on one data wire, whether
            they sit under the keys, under the case, or both.
          </p>
        </div>
        <label class="field">Pin the chain's data wire is soldered to
          <select v-model="pinDraft" class="input" data-test="light-pin">
            <option :value="null" disabled>Choose a pin</option>
            <option v-for="pin in freePins" :key="pin" :value="pin">{{ pinLabel(board, pin) }}</option>
          </select>
        </label>
        <label class="field">Number of lights on the chain
          <input v-model.number="countDraft" class="input" type="number" min="1" :max="LIMITS.leds" data-test="light-count" />
        </label>
        <p class="muted small">
          Unsure about the count? Start too high: extra ones do no harm, and you can lower it once you see which
          lights exist. Brightness starts limited so the lights cannot overload the USB port.
        </p>
        <button class="btn primary" :disabled="pinDraft === null || restarting" data-test="light-on" @click="turnOn">
          {{ restarting ? 'Restarting the keyboard…' : 'Switch the lights on' }}
        </button>
        <p class="muted small">The keyboard saves and restarts once, because the data pin is set up when it starts.</p>
      </template>

      <template v-else>
        <div class="col">
          <h2>Where the lights are</h2>
          <div class="modes row" data-test="light-mode">
            <button v-for="option in MODES" :key="option.id" class="mode" :class="{ on: mode === option.id }" :data-test="'mode-' + option.id" @click="setMode(option.id)">
              {{ option.name }}
            </button>
          </div>
          <p class="muted small">{{ MODES.find((option) => option.id === mode)?.hint }}</p>
          <template v-if="finding === undefined">
            <button class="btn small" data-test="place-lights" @click="placeByHand">
              {{ mode === 'underglow' ? 'Lights in another order? Place them by hand' : 'Place the lights one by one' }}
            </button>
          </template>
          <div v-else class="note col" data-test="placing">
            <strong>Light {{ finding + 1 }} of {{ lighting.leds.length }} is lit</strong>
            <template v-if="mode === 'both'">
              <label class="row check"><input v-model="asUnderglow" type="radio" :value="false" data-test="place-key" /> It shines up through a key</label>
              <label class="row check"><input v-model="asUnderglow" type="radio" :value="true" data-test="place-under" /> It shines out of the bottom (underglow)</label>
            </template>
            <span class="muted small">Click the key it sits under or nearest to. No light lit? The chain is shorter than {{ finding + 1 }}: lower the count below.</span>
            <div class="row wrap">
              <button class="btn small" :disabled="finding === 0" @click="find(finding - 1)">Back</button>
              <button class="btn small" data-test="place-skip" @click="find(finding + 1)">Skip</button>
              <button class="btn small ghost" data-test="place-done" @click="find(undefined)">Done</button>
            </div>
          </div>
        </div>

        <div class="col">
          <h2>What they show</h2>
          <p v-if="pinChanged" class="note warn" data-test="light-restart">
            The lights start working after a save and restart.
            <button class="btn small primary" :disabled="restarting" @click="saveAndRestart">Save and restart</button>
          </p>
          <div v-for="zone in zones" :key="zone.id" class="zone col" :data-test="'zone-' + zone.id">
            <h3>{{ zone.name }} <span class="muted">· {{ zone.count }}</span></h3>
            <select class="input" :value="lighting[zone.id].effect" :data-test="'effect-' + zone.id" @change="patchZone(zone.id, { effect: ($event.target as HTMLSelectElement).value as Effect })">
              <option v-for="effect in EFFECTS" :key="effect.id" :value="effect.id">{{ effect.name }}</option>
            </select>
            <span v-if="EFFECTS.find((e) => e.id === lighting[zone.id].effect)?.hint" class="muted small">
              {{ EFFECTS.find((e) => e.id === lighting[zone.id].effect)?.hint }}
            </span>
            <template v-if="lighting[zone.id].effect !== 'Off'">
              <label v-if="lighting[zone.id].effect !== 'Rainbow'" class="row between">Colour
                <input type="color" :value="zoneHex(lighting[zone.id])" :data-test="'colour-' + zone.id" @input="patchZone(zone.id, hexToHueSat(($event.target as HTMLInputElement).value))" />
              </label>
              <label class="field">Brightness
                <input type="range" min="0" max="255" :value="lighting[zone.id].val" :data-test="'brightness-' + zone.id" @input="patchZone(zone.id, { val: number($event) })" />
              </label>
              <label v-if="lighting[zone.id].effect === 'Breathing' || lighting[zone.id].effect === 'Rainbow'" class="field">Speed
                <input type="range" min="0" max="255" :value="lighting[zone.id].speed" @input="patchZone(zone.id, { speed: number($event) })" />
              </label>
            </template>
          </div>
        </div>

        <div class="col">
          <h2>The chain</h2>
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
          <label class="field">Power limit: {{ Math.round((lighting.limit / 255) * 100) }}% (about {{ peakMa }} mA with everything white)
            <input type="range" min="10" max="255" :value="lighting.limit" data-test="light-limit" @input="change({ limit: number($event) })" />
          </label>
          <p class="muted small">A USB port gives about 500 mA for the whole keyboard. Stay well under that.</p>
          <label class="row check">
            <input type="checkbox" :checked="lighting.order === 'Rgb'" data-test="light-order" @change="change({ order: lighting.order === 'Rgb' ? 'Grb' : 'Rgb' })" />
            Red and green are swapped on my lights
          </label>
          <button class="btn small ghost" data-test="light-remove" @click="removeLights">This keyboard has no lights</button>
        </div>
      </template>
    </aside>
  </div>
</template>

<style scoped>
.lighting {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 380px;
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
  padding: 16px;
  gap: 22px;
}

.small {
  font-size: 0.82rem;
}

.modes {
  gap: 0;
  border: 1px solid var(--line);
  border-radius: 10px;
  overflow: hidden;
}

.mode {
  flex: 1;
  padding: 8px 6px;
  border: none;
  background: var(--panel-2);
  cursor: pointer;
  transition: background 0.15s;
}

.mode + .mode {
  border-left: 1px solid var(--line);
}

.mode.on {
  background: var(--accent);
  color: var(--accent-ink);
  font-weight: 600;
}

.zone {
  gap: 8px;
  padding: 12px;
  border: 1px solid var(--line);
  border-radius: 12px;
}

.fields {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.between {
  justify-content: space-between;
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
