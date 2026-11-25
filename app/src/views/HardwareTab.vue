<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import BoardDiagram, { type PinRole } from '../components/BoardDiagram.vue'
import { attempt, toast } from '../lib/app'
import { planAddedKey } from '../lib/addkey'
import { findBoard, pinLabel, wirablePins } from '../lib/boards'
import { KeyCollector } from '../lib/collector'
import { desktop } from '../lib/desktop'
import { carriedVersion, isOlder, updateFirmware as updateByItself } from '../lib/firmware'
import { wiringParts } from '../lib/definition'
import type { Contact, HardwareConfig, MatrixConfig, Pin } from '../lib/protocol'
import { addKey, linkPins, refresh, role, setHardware, type Session } from '../lib/session'
import { VirtualTransport } from '../lib/transport'

const props = defineProps<{ session: Session }>()

const info = props.session.info
const hardware = computed(() => props.session.hardware)
const board = computed(() => findBoard(props.session.meta.controller, info.mcu, info.usable_pins))
const name = (pin: Pin) => pinLabel(board.value, pin)

const roles = computed(() => {
  const map = new Map<Pin, PinRole>()
  const { reserved_pins, encoders } = hardware.value
  reserved_pins.forEach((pin) => map.set(pin, 'reserved'))
  for (const part of wiringParts(hardware.value)) {
    if (part === 'None') continue
    if ('Direct' in part) part.Direct.pins.forEach((pin) => map.set(pin, 'direct'))
    else {
      part.Matrix.rows.forEach((pin) => map.set(pin, 'row'))
      part.Matrix.cols.forEach((pin) => map.set(pin, 'col'))
    }
  }
  encoders.forEach((e) => [e.a, e.b].forEach((pin) => map.set(pin, 'encoder')))
  const link = linkPins(hardware.value)
  if (link) [link.tx, link.rx].forEach((pin) => map.set(pin, 'link'))
  return map
})

/** One group of switches in a sentence. */
function describe(part: MatrixConfig): string {
  if (part === 'None') return ''
  if ('Direct' in part) {
    const { pins, active_high } = part.Direct
    const to = active_high ? ' to supply voltage' : ''
    return `${pins.length} ${pins.length === 1 ? 'key' : 'keys, each'} on its own pin${to} (${pins.map(name).join(', ')}).`
  }
  const { rows, cols, diodes } = part.Matrix
  const lines = `rows ${rows.map(name).join(', ')}; columns ${cols.map(name).join(', ')}`
  if (diodes === 'Both') return `Duplex matrix: ${lines}. Two keys per crossing; the chip switches the rows and reads the columns, then the other way round.`
  return `Matrix: ${lines}. The chip switches the ${diodes === 'Col2Row' ? 'rows' : 'columns'} and reads the ${diodes === 'Col2Row' ? 'columns' : 'rows'}.`
}
const summary = computed(() => wiringParts(hardware.value).map(describe).filter(Boolean).join(' ') || 'No keys set up.')

const change = (patch: Partial<HardwareConfig>) => attempt(() => setHardware(props.session, { ...hardware.value, ...patch }))

function patchEncoder(index: number, reverse: boolean, steps?: number) {
  const encoders = hardware.value.encoders.map((e, i) =>
    i !== index ? e : { a: reverse ? e.b : e.a, b: reverse ? e.a : e.b, steps_per_detent: steps ?? e.steps_per_detent },
  )
  return change({ encoders })
}

// --- A key that was missed -----------------------------------------------------
const adding = ref<'idle' | 'listening' | 'applying'>('idle')
const addProblem = ref('')
let collector: KeyCollector | undefined

/** Pins a key could be wired to: everything free for wiring, including the pins already in use for keys. */
const probePins = computed(() => {
  const { reserved_pins, encoders } = hardware.value
  const link = linkPins(hardware.value)
  const taken = new Set<Pin>([...reserved_pins, ...encoders.flatMap((e) => [e.a, e.b])])
  if (link) [link.tx, link.rx].forEach((pin) => taken.add(pin))
  const light = props.session.lighting.pin
  if (light !== null) taken.add(light)
  return wirablePins(board.value, info.usable_pins).filter((pin) => !taken.has(pin))
})

async function startAdding() {
  addProblem.value = ''
  collector = new KeyCollector()
  props.session.client.onProbe = onAddProbe
  const started = await attempt(async () => {
    await props.session.client.startProbe(probePins.value)
    return true
  })
  adding.value = started ? 'listening' : 'idle'
}

async function stopAdding() {
  props.session.client.onProbe = () => {}
  collector = undefined
  if (adding.value === 'idle') return
  adding.value = 'idle'
  await props.session.client.stopProbe().catch(() => {})
}

async function onAddProbe(contacts: Contact[]) {
  if (adding.value !== 'listening' || !collector) return
  collector.update(contacts)
  if (collector.confused) {
    collector.confused = false
    addProblem.value = 'That did not look like one key. Press only the missing key.'
    return
  }
  const key = collector.keys[0]
  if (!key) return
  collector.keys.splice(0)
  const plan = planAddedKey(hardware.value, key)
  if (!plan.ok) {
    addProblem.value = plan.problem
    return
  }
  const layout = props.session.layout
  if (!plan.rewired && layout[plan.pos] && layout[plan.pos].w > 0) {
    addProblem.value = 'That key is already part of the keyboard. Press the one that is missing.'
    return
  }
  adding.value = 'applying'
  props.session.client.onProbe = () => {}
  // Changing the wiring ends the detection by itself; otherwise end it here.
  const pos = await attempt(async () => {
    await props.session.client.stopProbe()
    return addKey(props.session, plan)
  })
  adding.value = 'idle'
  collector = undefined
  if (pos === undefined) return
  toast('Key added. Move it into place under Layout, and give it something to do under Keymap.')
}
onBeforeUnmount(stopAdding)

// --- Setup again / reset -------------------------------------------------------
const confirmSetup = ref(false)
const confirmReset = ref(false)

async function factoryReset() {
  const done = await attempt(async () => {
    await props.session.client.factoryReset()
    await refresh(props.session)
    props.session.unsaved = false
    return true
  })
  if (done) props.session.setup = true
}

// --- Firmware ------------------------------------------------------------------
const version = info.firmware.join('.')
const flashing = ref<'idle' | 'bootloader'>('idle')
const isVirtual = props.session.transport.kind === 'virtual'

const outdated = !isVirtual && isOlder(info.firmware, carriedVersion())

async function updateFirmware() {
  await attempt(async () => {
    // The desktop app does all of it; the keyboard comes back by itself.
    if (desktop && !isVirtual) return updateByItself(props.session, true)
    await props.session.client.reboot(true)
    flashing.value = 'bootloader'
    if (props.session.transport instanceof VirtualTransport) {
      // A virtual keyboard has no drive to copy to; play the steps through.
      await new Promise((resolve) => setTimeout(resolve, 1500))
      props.session.transport.finishFlash()
      await refresh(props.session)
      await props.session.client.watchKeys(true)
      flashing.value = 'idle'
      toast('Firmware updated; your settings were kept')
      return
    }
  })
}
</script>

<template>
  <div class="hardware">
    <div class="col">
      <div class="card col">
        <h2>Wiring</h2>
        <p data-test="wiring-summary">{{ summary }}</p>
        <p v-if="hardware.reserved_pins.length" class="muted">
          Protected, never touched: <span class="mono">{{ hardware.reserved_pins.map(name).join(', ') }}</span>
        </p>
        <p v-if="role(session) !== 'single'" class="muted">
          One half of a split keyboard. Either half can be the one plugged into the computer; this page shows
          this half's own wiring.
        </p>
        <label class="field">
          Ignore switch chatter shorter than {{ hardware.debounce_ms }} ms
          <input type="range" min="1" max="30" :value="hardware.debounce_ms" data-test="debounce" @change="change({ debounce_ms: Number(($event.target as HTMLInputElement).value) })" />
        </label>
        <p class="muted small">Raise this if one press sometimes types a letter twice.</p>
      </div>

      <div v-if="hardware.matrix !== 'None'" class="card col">
        <h2>A key is missing</h2>
        <p v-if="role(session) !== 'single'" class="muted" data-test="add-key-split">
          On a split keyboard, set the keys up again instead (below), with both halves plugged in: the two halves
          number their keys together, and adding one here would mix them up.
        </p>
        <template v-else>
          <p class="muted">
            For a key that was skipped during setup or soldered in later. The keymap and layout you have stay as
            they are.
          </p>
          <div v-if="adding === 'idle'" class="row wrap">
            <button class="btn" data-test="add-key" @click="startAdding">Add a missed key</button>
          </div>
          <div v-else class="note col" data-test="add-key-listening">
            <strong>{{ adding === 'applying' ? 'Adding the key…' : 'Press the missing key once.' }}</strong>
            <span class="muted small">The keyboard does not type while it listens for the new key.</span>
            <div class="row"><button class="btn small ghost" data-test="add-key-cancel" :disabled="adding === 'applying'" @click="stopAdding">Cancel</button></div>
          </div>
          <p v-if="addProblem" class="note" data-test="add-key-problem">{{ addProblem }}</p>
        </template>
      </div>

      <div v-if="hardware.encoders.length" class="card col">
        <h2>Knobs</h2>
        <div v-for="(encoder, index) in hardware.encoders" :key="index" class="row">
          <span class="chip">knob {{ index + 1 }}</span>
          <span class="mono">{{ name(encoder.a) }} + {{ name(encoder.b) }}</span>
          <button class="btn small" :data-test="'reverse-' + index" @click="patchEncoder(index, true)">Reverse direction</button>
          <label class="row small">
            <select class="input" :value="encoder.steps_per_detent" @change="patchEncoder(index, false, Number(($event.target as HTMLSelectElement).value))">
              <option :value="4">normal</option>
              <option :value="2">half-step knob</option>
              <option :value="1">every signal change</option>
            </select>
          </label>
        </div>
        <p class="muted small">If one click registers twice or only every second click, try another knob type.</p>
      </div>

      <div class="card col">
        <h2>Firmware</h2>
        <p>
          Version <strong>{{ version }}</strong> on {{ info.mcu }}.
          <span v-if="outdated" class="muted" data-test="firmware-outdated">This app carries {{ carriedVersion().join('.') }}.</span>
        </p>
        <Transition name="rise" mode="out-in">
          <div v-if="flashing === 'bootloader'" key="boot" class="note">
            <strong>The keyboard is in update mode.</strong>
            <template v-if="isVirtual"> Copying the new firmware…</template>
            <template v-else-if="!desktop">
              It now shows up as a drive (RPI-RP2 or similar). Copy the JMK firmware file onto it; the keyboard
              restarts by itself. Your settings are kept.
            </template>
          </div>
          <button v-else class="btn" :class="{ primary: outdated }" title="The keyboard is away for a few seconds and comes back by itself. Your settings are kept." data-test="update-firmware" @click="updateFirmware">
            {{ outdated || !desktop || isVirtual ? 'Update firmware' : 'Install the firmware again' }}
          </button>
        </Transition>
      </div>

      <div class="card col">
        <h2>Start over</h2>
        <div class="row wrap">
          <button v-if="!confirmSetup" class="btn" data-test="rerun" @click="confirmSetup = true">Find the keys again</button>
          <template v-else>
            <span>The keymap is replaced with starter letters.</span>
            <button class="btn primary" data-test="rerun-confirm" @click="session.setup = true">Run setup</button>
            <button class="btn ghost" @click="confirmSetup = false">Cancel</button>
          </template>
        </div>
        <div class="row wrap">
          <button v-if="!confirmReset" class="btn danger" data-test="factory-reset" @click="confirmReset = true">Erase everything</button>
          <template v-else>
            <span>Wiring, layout and keymap are erased from the keyboard.</span>
            <button class="btn danger" data-test="factory-reset-confirm" @click="factoryReset">Erase</button>
            <button class="btn ghost" @click="confirmReset = false">Cancel</button>
          </template>
        </div>
      </div>
    </div>

    <div class="card diagram">
      <BoardDiagram :board="board" :roles="roles" />
      <div class="legend row">
        <span class="chip"><i style="background: var(--accent)" />rows</span>
        <span class="chip"><i style="background: #6ea8ff" />columns</span>
        <span class="chip"><i style="background: var(--ok)" />one key each</span>
        <span class="chip"><i style="background: #c58bff" />knob</span>
        <span class="chip"><i style="background: #ff8fb1" />cable</span>
        <span class="chip"><i style="background: var(--danger)" />protected</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.hardware {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(280px, 420px);
  gap: 18px;
  align-items: start;
}

.hardware > .col {
  gap: 18px;
}

.small {
  font-size: 0.85rem;
}

.wrap {
  flex-wrap: wrap;
}

input[type='range'] {
  accent-color: var(--accent);
}

.diagram {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}

.legend {
  flex-wrap: wrap;
  justify-content: center;
}

.legend i {
  width: 9px;
  height: 9px;
  border-radius: 50%;
}

@media (max-width: 900px) {
  .hardware {
    grid-template-columns: 1fr;
  }
}
</style>
