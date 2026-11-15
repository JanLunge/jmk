<script setup lang="ts">
// A controller board seen from above, USB connector at the top. Pads can be
// coloured by what they are used for and light up when current flows.
import { computed } from 'vue'
import type { Board, Pad } from '../lib/boards'
import type { Pin } from '../lib/protocol'

export type PinRole = 'row' | 'col' | 'direct' | 'encoder' | 'link' | 'reserved' | 'candidate'

const props = defineProps<{
  board: Board
  roles?: Map<Pin, PinRole>
  /** Pins with a connection right now. */
  active?: Set<Pin>
  selectable?: boolean
}>()

const emit = defineEmits<{ toggle: [pin: Pin] }>()

const PITCH = 24
const rows = computed(() => Math.max(props.board.left.length, props.board.right.length))
const width = computed(() => props.board.width * PITCH)
const height = computed(() => (rows.value + 1) * PITCH + (props.board.bottom ? PITCH : 0))
const internal = computed(() => new Map(props.board.internal.map((i) => [i.pin, i.use])))

interface Drawn {
  pad: Pad
  x: number
  y: number
  labelX: number
  labelY: number
  anchor: 'start' | 'end' | 'middle'
}

const pads = computed<Drawn[]>(() => {
  const side = (list: Pad[], x: number, labelX: number, anchor: 'start' | 'end') =>
    list.map((pad, i) => ({ pad, x, y: (i + 1) * PITCH, labelX, labelY: (i + 1) * PITCH, anchor }))
  const bottom = (props.board.bottom ?? []).map((pad, i, all) => {
    const x = width.value / 2 + (i - (all.length - 1) / 2) * PITCH
    const y = height.value - PITCH / 2
    return { pad, x, y, labelX: x, labelY: y + 19, anchor: 'middle' as const }
  })
  return [
    ...side(props.board.left, PITCH / 2, -10, 'end'),
    ...side(props.board.right, width.value - PITCH / 2, width.value + 10, 'start'),
    ...bottom,
  ]
})

function roleOf(pad: Pad): string {
  if (pad.pin === undefined) return pad.kind
  if (internal.value.has(pad.pin)) return 'internal'
  return props.roles?.get(pad.pin) ?? 'free'
}

function title(pad: Pad): string {
  if (pad.pin === undefined) return pad.label
  const use = internal.value.get(pad.pin)
  return use ? `${pad.label}: ${use}` : `${pad.label} (chip pin ${pad.pin})`
}

function click(pad: Pad) {
  if (props.selectable && pad.pin !== undefined && !internal.value.has(pad.pin)) emit('toggle', pad.pin)
}
</script>

<template>
  <svg
    class="board"
    :viewBox="`-78 -22 ${width + 156} ${height + 50}`"
    :style="{ maxWidth: width + 156 + 'px' }"
    role="img"
    :aria-label="board.name"
  >
    <rect class="pcb" x="0" y="0" :width="width" :height="height" rx="9" />
    <rect class="usb" :x="width / 2 - 20" y="-10" width="40" height="26" rx="4" />
    <rect class="chip-body" :x="width / 2 - 26" :y="height / 2 - 26" width="52" height="52" rx="5" />
    <text class="chip-name" :x="width / 2" :y="height / 2">{{ board.mcu.toUpperCase() }}</text>
    <g
      v-for="{ pad, x, y, labelX, labelY, anchor } in pads"
      :key="pad.label + x + y"
      class="pad"
      :class="[
        'role-' + roleOf(pad),
        { active: pad.pin !== undefined && active?.has(pad.pin), clickable: selectable && pad.pin !== undefined && roleOf(pad) !== 'internal' },
      ]"
      :data-pin="pad.pin"
      @click="click(pad)"
    >
      <title>{{ title(pad) }}</title>
      <circle class="halo" :cx="x" :cy="y" r="13" />
      <circle class="hole" :cx="x" :cy="y" r="7.5" />
      <text :x="labelX" :y="labelY" :text-anchor="anchor">{{ pad.label }}</text>
    </g>
  </svg>
</template>

<style scoped>
.board {
  display: block;
  width: 100%;
  height: auto;
  user-select: none;
}

.pcb {
  fill: #14532d;
  stroke: #0c3a1f;
  stroke-width: 2;
}

.usb {
  fill: #b8bcc4;
  stroke: #7c828d;
}

.chip-body {
  fill: #101214;
}

.chip-name {
  fill: #6b7280;
  font-size: 8px;
  text-anchor: middle;
  dominant-baseline: central;
  letter-spacing: 0.05em;
}

text {
  fill: var(--muted);
  font-size: 10.5px;
  dominant-baseline: central;
  font-family: ui-monospace, Menlo, Consolas, monospace;
  transition: fill 0.2s;
}

.hole {
  fill: #d4a017;
  stroke: #8a6a10;
  stroke-width: 1.5;
  transition:
    fill 0.2s,
    stroke 0.2s;
}

.halo {
  fill: var(--signal);
  opacity: 0;
  transform-box: fill-box;
  transform-origin: center;
  transition: opacity 0.2s;
}

.clickable {
  cursor: pointer;
}

.clickable:hover .hole {
  stroke: var(--text);
}

.role-ground .hole,
.role-power .hole,
.role-other .hole {
  fill: #6b7280;
  stroke: #4b5563;
}

.role-internal .hole {
  fill: #374151;
  stroke: #1f2937;
}

.role-candidate .hole {
  fill: #f3d27a;
}

.role-row .hole {
  fill: var(--accent);
  stroke: var(--accent);
}

.role-col .hole {
  fill: #6ea8ff;
  stroke: #6ea8ff;
}

.role-direct .hole {
  fill: var(--ok);
  stroke: var(--ok);
}

.role-encoder .hole {
  fill: #c58bff;
  stroke: #c58bff;
}

.role-link .hole {
  fill: #ff8fb1;
  stroke: #ff8fb1;
}

.role-reserved .hole {
  fill: var(--danger);
  stroke: var(--danger);
}

.role-row text,
.role-col text,
.role-direct text,
.role-encoder text,
.role-link text,
.role-reserved text {
  fill: var(--text);
}

.active .halo {
  opacity: 0.55;
  animation: pulse 0.9s ease-out infinite;
}

.active .hole {
  fill: var(--signal);
  stroke: var(--signal);
}

.active text {
  fill: var(--signal);
}

@keyframes pulse {
  from {
    transform: scale(0.7);
    opacity: 0.7;
  }
  to {
    transform: scale(1.5);
    opacity: 0;
  }
}
</style>
