<script setup lang="ts">
// The workbench for a virtual keyboard: its boards as they lie on the desk,
// the cables that can be pulled and the computer they type into. Keys are
// clicked to "press" them.
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { app, plugCable, plugUsb, type Bench } from '../lib/app'
import { charFor, keyLabel, modsLabel } from '../lib/keycodes'
import { fromWire, shown } from '../lib/lighting'
import { bounds, hasKey } from '../lib/layout'
import { pinLabel } from '../lib/boards'
import { role, type Session } from '../lib/session'

const props = defineProps<{
  bench: Bench
  /** The keyboard is being set up, which is done by pressing its keys: show them. */
  setup: boolean
}>()

// In the editor the bench folds to a strip of its plugs, out of the way.
const open = ref(props.setup)
watch(
  () => props.setup,
  (now) => (open.value = now),
)
/** Keys held down, as "half:key". */
const held = ref(new Set<string>())
const split = computed(() => props.bench.halves.length === 2)
const sessionOf = (index: number): Session | undefined => {
  const id = props.bench.halves[index].sessionId
  return app.sessions.find((session) => session.id === id && !session.closed)
}
const powered = (index: number) => {
  const half = props.bench.halves[index]
  const other = props.bench.halves[1 - index]
  return half.usb || (props.bench.cable && !!other?.usb)
}

function press(index: number, key: number, event: PointerEvent) {
  const id = `${index}:${key}`
  // Shift-click latches a key down so two can be held with one mouse.
  if (event.shiftKey && held.value.has(id)) return release(index, key)
  held.value.add(id)
  props.bench.halves[index].transport.setKey(key, true)
  if (!event.shiftKey) window.addEventListener('pointerup', () => release(index, key), { once: true })
}

function release(index: number, key: number) {
  held.value.delete(`${index}:${key}`)
  props.bench.halves[index].transport.setKey(key, false)
}

onBeforeUnmount(() => held.value.forEach((id) => release(...(id.split(':').map(Number) as [number, number]))))

// --- The picture ---------------------------------------------------------------
const SCALE = 0.3
const PAD = 12
const TOP = 84
const scene = computed(() => {
  const boxes = props.bench.halves.map((half) => bounds(half.demo.keys))
  const sizes = boxes.map((box) => ({ w: box.w * SCALE + 2 * PAD, h: box.h * SCALE + 2 * PAD }))
  const bodyHeight = Math.max(...sizes.map((size) => size.h))
  const width = split.value ? sizes[0].w + sizes[1].w + 260 : Math.max(sizes[0].w + 120, 420)
  const centre = width / 2
  const halves = props.bench.halves.map((half, index) => {
    const { w, h } = sizes[index]
    const x = !split.value ? centre - w / 2 : index === 0 ? 30 : width - 30 - w
    const y = TOP + (bodyHeight - h) / 2
    // The controller sits at the top, on the side facing the other half.
    const chip = !split.value ? x + w / 2 - 16 : index === 0 ? x + w - 46 : x + 14
    const port = { x: chip + 16, y: y - 10 }
    const socket = { x: !split.value ? centre : centre + (index === 0 ? -70 : 70), y: 40 }
    const keys = half.demo.keys.map((key, pos) => ({
      pos,
      key,
      x: x + PAD + (key.x - boxes[index].x) * SCALE,
      y: y + PAD + (key.y - boxes[index].y) * SCALE,
      w: key.w * SCALE - 3,
      h: key.h * SCALE - 3,
    })).filter(({ key }) => hasKey(key))
    return {
      index,
      x,
      y,
      w,
      h,
      chip,
      keys,
      wire: `M${port.x},${port.y} C${port.x},${port.y - 26} ${socket.x},${socket.y + 26} ${socket.x},${socket.y}`,
      plug: { x: (port.x + socket.x) / 2, y: (port.y + socket.y) / 2 },
    }
  })
  const mid = TOP + bodyHeight / 2
  const cable = split.value
    ? { path: `M${halves[0].x + halves[0].w},${mid} C${centre - 50},${mid + 44} ${centre + 50},${mid + 44} ${halves[1].x},${mid}`, x: centre, y: mid + 33 }
    : undefined
  return { width, height: TOP + bodyHeight + 30, halves, cable, labelY: TOP + bodyHeight + 20 }
})

/** One line under each board on what it is doing. */
function stateOf(index: number): string {
  const half = props.bench.halves[index]
  if (!half.firmware) return half.installing !== undefined ? 'installing JMK…' : half.usb ? 'new board · no firmware yet' : 'not plugged in'
  if (!powered(index)) return 'no power'
  const session = sessionOf(index)
  // Its USB cable is in, but the computer does not have it as a keyboard yet.
  if (half.usb && !session && !half.sessionId) return 'restarting as a keyboard…'
  const other = split.value ? sessionOf(1 - index) : undefined
  if (session?.setup || (!session && other?.setup)) return 'JMK running · being set up'
  if (!split.value) return session ? 'on USB' : 'running'
  // A half without USB is known through the half that has it.
  if (session && role(session) !== 'single') {
    if (!session.split.connected) return 'types its own keys'
    return session.split.leading ? 'types for both' : 'follows the other half'
  }
  if (!session && other && role(other) !== 'single' && other.split.connected) return other.split.leading ? 'follows the other half' : 'types for both'
  return session ? 'on USB' : 'powered through the cable'
}

const led = (index: number) => {
  const half = props.bench.halves[index]
  if (!half.firmware) return half.usb ? '#f2c14e' : '#444'
  return powered(index) ? 'var(--signal)' : '#444'
}

// Which half types is asked from the halves themselves, so the bench shows
// what the keyboard does and not what the app expects.
let timer: ReturnType<typeof setInterval> | undefined
async function look() {
  for (const index of props.bench.halves.keys()) {
    const session = sessionOf(index)
    if (!session || session.setup || role(session) === 'single') continue
    try {
      session.split = await session.client.splitStatus()
    } catch {
      // Unplugged in this very moment.
    }
  }
}
onMounted(() => (timer = setInterval(look, 700)))
onBeforeUnmount(() => clearInterval(timer))

function plugAll() {
  if (!props.bench.cable) plugCable(props.bench, true)
  props.bench.halves.forEach((half, index) => !half.usb && plugUsb(props.bench, index, true))
}
const allPlugged = computed(() => props.bench.cable && props.bench.halves.every((half) => half.usb))

// --- Knobs and lights, for the boards that have them -----------------------------
const knobs = computed(() =>
  props.bench.halves.flatMap((half, index) => half.demo.wiring.encoders.map((_, encoder) => ({ index, encoder }))),
)
const strip = computed(() => {
  const half = props.bench.halves[0]
  const lights = half.demo.lights
  const session = sessionOf(0)
  if (!lights) return undefined
  // Lit once the app has been told the pin.
  const live = session && session.lighting.pin === lights.pin ? session.lights : []
  const order = session?.lighting.order ?? 'Grb'
  return { pin: lights.pin, colors: [...Array(lights.count).keys()].map((n) => (live[n] ? shown(fromWire(order, live[n])) : '#000000')) }
})

// What the computer has received, as the text it would produce where possible.
const typed = computed(() => {
  const out: string[] = []
  const previous = new Map<number, number[]>()
  for (const { half, event } of props.bench.typed) {
    if ('Consumer' in event) {
      if (event.Consumer) out.push('♪')
      continue
    }
    const before = previous.get(half) ?? []
    const fresh = event.Keyboard.keys.filter((k) => !before.includes(k))
    previous.set(half, event.Keyboard.keys)
    for (const usage of fresh) {
      const shift = (event.Keyboard.mods & 0x22) !== 0
      const others = event.Keyboard.mods & ~0x22
      const char = charFor(usage, shift)
      out.push((others ? modsLabel(others) + '+' : '') + (char ?? `⟨${keyLabel(usage)}⟩`))
    }
  }
  return out.slice(-26).join('')
})
</script>

<template>
  <aside class="dock" :class="{ open }" data-test="virtual-board">
    <div class="handle row">
      <button class="fold row" @click="open = !open">
        <span class="chip signal">virtual</span>
        <strong>{{ bench.demo.name }}</strong>
        <span v-if="open" class="muted hint">click keys to press them · shift-click to hold · click a plug to pull it</span>
      </button>
      <template v-if="!open">
        <template v-for="(half, index) in bench.halves" :key="index">
          <button class="mini" :class="{ cut: !half.usb }" :data-test="'usb-' + index" :title="half.usb ? 'Pull this USB cable' : 'Plug this USB cable in'" @click="plugUsb(bench, index, !half.usb)">
            {{ split ? half.demo.label.toLowerCase().replace(' half', '') + ' ' : '' }}USB
          </button>
          <button v-if="split && index === 0" class="mini" :class="{ cut: !bench.cable }" data-test="cable" :title="bench.cable ? 'Pull the cable between the halves' : 'Plug it back in'" @click="plugCable(bench, !bench.cable)">
            cable
          </button>
        </template>
        <span v-for="knob in knobs" :key="knob.index + ':' + knob.encoder" class="row knob mini-knob">
          <button class="btn small" :data-test="'knob-ccw-' + knob.encoder" @click="bench.halves[knob.index].transport.turnEncoder(knob.encoder, false)">⟲</button>
          <span class="muted small">knob</span>
          <button class="btn small" :data-test="'knob-cw-' + knob.encoder" @click="bench.halves[knob.index].transport.turnEncoder(knob.encoder, true)">⟳</button>
        </span>
        <span v-if="strip" class="row strip" data-test="strip">
          <span v-for="(color, index) in strip.colors" :key="index" class="bulb" :style="{ background: color, boxShadow: color === '#000000' ? 'none' : `0 0 10px ${color}` }" :data-color="color" />
        </span>
        <span class="muted small received">received: <span class="mono" data-test="typed">{{ typed }}</span></span>
      </template>
      <span class="grow" />
      <button v-if="!allPlugged" class="btn small" data-test="plug-all" @click="plugAll">Plug everything in</button>
      <button class="btn small ghost" data-test="bench-toggle" @click="open = !open">{{ open ? 'hide' : 'show keys' }}</button>
    </div>
    <div v-if="open" class="body">
      <div class="desk" :style="{ width: scene.width + 'px' }">
        <div class="screen">
          <span class="muted label">the computer received</span>
          <div class="typed mono" data-test="typed">{{ typed }}<span class="caret" /></div>
        </div>
        <svg :viewBox="`0 0 ${scene.width} ${scene.height}`" :width="scene.width" :height="scene.height">
          <path v-for="half in scene.halves" :key="'w' + half.index" class="wire" :class="{ cut: !bench.halves[half.index].usb }" :d="half.wire" />
          <path
            v-if="scene.cable"
            class="wire"
            :class="{ cut: !bench.cable, live: bench.cable && (stateOf(0).startsWith('types for') || stateOf(1).startsWith('types for')) }"
            :d="scene.cable.path"
          />
          <g v-for="half in scene.halves" :key="half.index" class="board" :class="{ dead: !powered(half.index) || !bench.halves[half.index].firmware }" :data-test="'half-' + half.index">
            <rect class="case" :x="half.x" :y="half.y" :width="half.w" :height="half.h" rx="14" />
            <rect class="pcb" :x="half.chip" :y="half.y - 10" width="32" height="16" rx="3" />
            <circle :cx="half.chip + 25" :cy="half.y - 2" r="3" :fill="led(half.index)" />
            <rect
              v-for="{ pos, key, x, y, w, h } in half.keys"
              :key="pos"
              class="key"
              :class="{ down: held.has(half.index + ':' + pos) }"
              :x="x"
              :y="y"
              :width="w"
              :height="h"
              rx="6"
              :transform="`rotate(${key.r / 10} ${x + w / 2} ${y + h / 2})`"
              @pointerdown="press(half.index, pos, $event)"
            />
            <text class="state" :x="half.x + half.w / 2" :y="scene.labelY" text-anchor="middle" :data-test="'state-' + half.index">
              <template v-if="split">{{ bench.halves[half.index].demo.label.toLowerCase() }} · </template>{{ stateOf(half.index) }}
            </text>
          </g>
          <g
            v-for="half in scene.halves"
            :key="'p' + half.index"
            class="plug"
            :class="{ cut: !bench.halves[half.index].usb }"
            :data-test="'usb-' + half.index"
            @click="plugUsb(bench, half.index, !bench.halves[half.index].usb)"
          >
            <title>{{ bench.halves[half.index].usb ? 'Pull this USB cable' : 'Plug this USB cable into the computer' }}</title>
            <rect :x="half.plug.x - (bench.halves[half.index].usb ? 24 : 52)" :y="half.plug.y - 11" :width="bench.halves[half.index].usb ? 48 : 104" height="22" rx="11" />
            <text :x="half.plug.x" :y="half.plug.y + 4" text-anchor="middle">{{ bench.halves[half.index].usb ? 'USB' : 'USB unplugged' }}</text>
          </g>
          <g v-if="scene.cable" class="plug" :class="{ cut: !bench.cable }" data-test="cable" @click="plugCable(bench, !bench.cable)">
            <title>{{ bench.cable ? 'Pull the cable between the halves' : 'Plug the cable between the halves back in' }}</title>
            <rect :x="scene.cable.x - (bench.cable ? 30 : 56)" :y="scene.cable.y - 11" :width="bench.cable ? 60 : 112" height="22" rx="11" />
            <text :x="scene.cable.x" :y="scene.cable.y + 4" text-anchor="middle">{{ bench.cable ? 'cable' : 'cable unplugged' }}</text>
          </g>
        </svg>
      </div>
      <div v-if="knobs.length || strip" class="side col">
        <div v-for="knob in knobs" :key="knob.index + ':' + knob.encoder" class="row knob">
          <button class="btn small" :data-test="'knob-ccw-' + knob.encoder" @click="bench.halves[knob.index].transport.turnEncoder(knob.encoder, false)">⟲</button>
          <span class="muted">knob {{ knob.encoder + 1 }}</span>
          <button class="btn small" :data-test="'knob-cw-' + knob.encoder" @click="bench.halves[knob.index].transport.turnEncoder(knob.encoder, true)">⟳</button>
        </div>
        <div v-if="strip" class="row strip" data-test="strip">
          <span v-for="(color, index) in strip.colors" :key="index" class="bulb" :style="{ background: color, boxShadow: color === '#000000' ? 'none' : `0 0 10px ${color}` }" :data-color="color" />
          <span class="muted">lights on {{ pinLabel(undefined, strip.pin) }}</span>
        </div>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.strip {
  gap: 6px;
}

.bulb {
  width: 14px;
  height: 14px;
  border-radius: 50%;
  border: 1px solid var(--line);
}

.dock {
  flex: none;
  border-top: 1px solid var(--line);
  background: var(--panel);
}

.handle {
  gap: 8px;
  padding: 6px 18px;
}

.fold {
  gap: 10px;
  background: none;
  border: none;
  cursor: pointer;
  text-align: left;
  padding: 2px 0;
  color: inherit;
  font: inherit;
}

.hint {
  font-size: 0.82rem;
}

.mini {
  font: inherit;
  font-size: 0.75rem;
  font-weight: 600;
  padding: 1px 10px;
  border-radius: 99px;
  border: 1px solid var(--muted);
  background: none;
  color: var(--text);
  cursor: pointer;
}

.mini.cut {
  border-style: dashed;
  border-color: var(--accent);
  color: var(--accent);
}

.received {
  margin-left: 6px;
}

.mini-knob {
  gap: 4px;
}

.body {
  display: flex;
  gap: 24px;
  padding: 0 18px 10px;
  align-items: center;
  justify-content: center;
  overflow-x: auto;
}

.desk {
  position: relative;
  flex: none;
}

.desk svg {
  display: block;
}

.screen {
  position: absolute;
  left: 50%;
  top: 0;
  width: 250px;
  height: 40px;
  transform: translateX(-50%);
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 2px 12px;
  background: var(--bg);
  overflow: hidden;
}

.label {
  display: block;
  font-size: 0.68rem;
  line-height: 1.3;
}

.typed {
  line-height: 1.3;
  white-space: nowrap;
}

.case {
  fill: var(--bg);
  stroke: var(--line);
}

.pcb {
  fill: #3a3d42;
  stroke: #5a5e66;
}

.key {
  fill: var(--cap-top);
  stroke: var(--cap);
  stroke-width: 1.5;
  cursor: pointer;
}

.key:hover {
  filter: brightness(1.15);
}

.key.down {
  fill: var(--accent);
}

.board.dead .key {
  opacity: 0.45;
}

.state {
  fill: var(--muted);
  font-size: 12px;
}

.wire {
  fill: none;
  stroke: var(--muted);
  stroke-width: 3;
  stroke-linecap: round;
}

.wire.live {
  stroke: var(--signal);
}

.wire.cut {
  stroke: var(--line);
  stroke-dasharray: 3 7;
}

.plug {
  cursor: pointer;
}

.plug rect {
  fill: var(--panel);
  stroke: var(--muted);
  stroke-width: 1.2;
}

.plug text {
  fill: var(--text);
  font-size: 11px;
  font-weight: 600;
}

.plug.cut rect {
  stroke: var(--accent);
  stroke-dasharray: 3 3;
}

.plug.cut text {
  fill: var(--accent);
}

.plug:hover rect {
  fill: var(--panel-2);
}

.side {
  width: 260px;
  flex: none;
}

.caret {
  display: inline-block;
  width: 7px;
  height: 1em;
  margin-left: 1px;
  vertical-align: text-bottom;
  background: var(--signal);
  animation: blink 1s steps(2) infinite;
}

@keyframes blink {
  50% {
    opacity: 0;
  }
}
</style>
