<script setup lang="ts">
import { computed, ref } from 'vue'
import BoardDiagram, { type PinRole } from '../components/BoardDiagram.vue'
import { attempt, toast } from '../lib/app'
import { findBoard, pinLabel } from '../lib/boards'
import { desktop } from '../lib/desktop'
import type { HardwareConfig, Pin } from '../lib/protocol'
import { registryMcu } from '../lib/registry'
import { refresh, role, setHardware, type Session } from '../lib/session'
import { VirtualTransport } from '../lib/transport'

const props = defineProps<{ session: Session }>()

const info = props.session.info
const hardware = computed(() => props.session.hardware)
const board = computed(() => findBoard(props.session.meta.controller, info.mcu, info.usable_pins))
const name = (pin: Pin) => pinLabel(board.value, pin)

const roles = computed(() => {
  const map = new Map<Pin, PinRole>()
  const { matrix, reserved_pins, encoders, split } = hardware.value
  reserved_pins.forEach((pin) => map.set(pin, 'reserved'))
  if (matrix !== 'None' && 'Direct' in matrix) matrix.Direct.pins.forEach((pin) => map.set(pin, 'direct'))
  if (matrix !== 'None' && 'Matrix' in matrix) {
    matrix.Matrix.rows.forEach((pin) => map.set(pin, 'row'))
    matrix.Matrix.cols.forEach((pin) => map.set(pin, 'col'))
  }
  encoders.forEach((e) => [e.a, e.b].forEach((pin) => map.set(pin, 'encoder')))
  const link = split === 'None' ? undefined : 'Central' in split ? split.Central : split.Peripheral
  if (link && link !== 'Ble') [link.Uart.tx, link.Uart.rx].forEach((pin) => map.set(pin, 'link'))
  return map
})

const summary = computed(() => {
  const matrix = hardware.value.matrix
  if (matrix === 'None') return 'No keys set up.'
  if ('Direct' in matrix) return `${matrix.Direct.pins.length} keys, each on its own pin (${matrix.Direct.pins.map(name).join(', ')}).`
  const { rows, cols, diodes } = matrix.Matrix
  return `Matrix: rows ${rows.map(name).join(', ')}; columns ${cols.map(name).join(', ')}. The chip switches the ${diodes === 'Col2Row' ? 'rows' : 'columns'} and reads the ${diodes === 'Col2Row' ? 'columns' : 'rows'}.`
})

const change = (patch: Partial<HardwareConfig>) => attempt(() => setHardware(props.session, { ...hardware.value, ...patch }))

function patchEncoder(index: number, reverse: boolean, steps?: number) {
  const encoders = hardware.value.encoders.map((e, i) =>
    i !== index ? e : { a: reverse ? e.b : e.a, b: reverse ? e.a : e.b, steps_per_detent: steps ?? e.steps_per_detent },
  )
  return change({ encoders })
}

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
const flashing = ref<'idle' | 'bootloader' | 'done'>('idle')
const isVirtual = props.session.transport.kind === 'virtual'

async function updateFirmware() {
  await attempt(async () => {
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
    if (desktop) {
      const drive = await desktop.flash(registryMcu(info.mcu))
      toast(`Firmware copied to ${drive}. The keyboard restarts by itself.`)
      flashing.value = 'done'
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
          Split keyboard, this is the {{ role(session) === 'central' ? 'half that plugs into the computer' : 'other half' }}.
          A changed cable setting takes effect after saving and replugging.
        </p>
        <label class="field">
          Ignore switch chatter for {{ hardware.debounce_ms }} ms after each change
          <input type="range" min="1" max="30" :value="hardware.debounce_ms" data-test="debounce" @change="change({ debounce_ms: Number(($event.target as HTMLInputElement).value) })" />
        </label>
        <p class="muted small">Raise this if one press sometimes types a letter twice.</p>
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
        <p>Version <strong>{{ version }}</strong> on {{ info.mcu }}.</p>
        <Transition name="rise" mode="out-in">
          <div v-if="flashing === 'bootloader'" key="boot" class="note">
            <strong>The keyboard is in update mode.</strong>
            <template v-if="isVirtual"> Copying the new firmware…</template>
            <template v-else-if="!desktop">
              It now shows up as a drive (RPI-RP2 or similar). Copy the JMK firmware file onto it; the keyboard
              restarts by itself. Your settings are kept.
            </template>
          </div>
          <div v-else-if="flashing === 'done'" key="done" class="note">Done. Connect the keyboard again from the home screen.</div>
          <button v-else class="btn" data-test="update-firmware" @click="updateFirmware">Update firmware</button>
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
