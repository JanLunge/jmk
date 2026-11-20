<script setup lang="ts">
// A controller at thumbnail size: enough to see pins light up as keys are
// pressed, as a nod to how the wiring is found. Clicking it asks for the
// full picture.
import { computed } from 'vue'
import type { Board, Pad } from '../lib/boards'
import type { Pin } from '../lib/protocol'

const props = defineProps<{
  board: Board
  /** Pins that are connected right now. */
  active?: Pin[]
  /** Pins set aside for something else: the cable between halves, protected ones. */
  taken?: Pin[]
}>()

// Drawn to the board's real proportions: pads a pitch apart, the board as
// many pitches wide as it is.
const PITCH = 5
const rows = computed(() => Math.max(props.board.left.length, props.board.right.length))
const width = computed(() => props.board.width * PITCH)
const height = computed(() => rows.value * PITCH + 6)
const dots = computed(() => {
  const side = (pads: Pad[], x: number) => pads.map((pad, index) => ({ pad, x, y: 5.5 + index * PITCH }))
  return [...side(props.board.left, 3), ...side(props.board.right, width.value - 3)]
})
const color = (pad: Pad) => {
  if (pad.pin === undefined) return 'var(--line)'
  if (props.active?.includes(pad.pin)) return 'var(--signal)'
  if (props.taken?.includes(pad.pin)) return '#ff8fab'
  return '#f2c14e'
}
</script>

<template>
  <svg class="thumb" :viewBox="`0 0 ${width} ${height}`" :height="Math.min(height, 78)" aria-hidden="true">
    <rect x="0.5" y="1.5" :width="width - 1" :height="height - 2" rx="2.5" class="pcb" />
    <rect :x="width / 2 - 5" y="0" width="10" height="5" rx="1" class="usb" />
    <rect :x="width / 2 - 6" :y="height * 0.5" width="12" height="12" rx="1" class="chip" />
    <circle v-for="(dot, index) in dots" :key="index" :cx="dot.x" :cy="dot.y" :r="active?.includes(dot.pad.pin ?? -1) ? 2.4 : 1.7" :fill="color(dot.pad)" />
  </svg>
</template>

<style scoped>
.thumb {
  flex: none;
  display: block;
}

.pcb {
  fill: #3a3d42;
  stroke: #5a5e66;
}

.usb {
  fill: #b9bcc4;
}

.chip {
  fill: #101010;
}

circle {
  transition: r 0.15s var(--ease), fill 0.15s var(--ease);
}
</style>
