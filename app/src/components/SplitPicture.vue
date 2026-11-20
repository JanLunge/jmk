<script setup lang="ts">
// How far a keyboard has got on its way into the app, as one picture that
// changes shape instead of being swapped: with nothing plugged in it offers
// both kinds of keyboard; one board glides to the middle on its own; two
// take their places as halves, with the cable between them.
import { computed } from 'vue'

export interface PictureBoard {
  label: string
  /** absent: not seen yet. new: seen, no firmware. old: firmware too old for this. */
  state: 'absent' | 'new' | 'installing' | 'old' | 'ready' | 'unplugged'
  /** A line under the board; a default per state otherwise. */
  note?: string
  progress?: number
}

const props = defineProps<{
  /** None while nothing is plugged in: the picture then shows both kinds of keyboard. */
  boards: PictureBoard[]
  /** The cable between two boards: not looked for, being looked for, found, or looked for in vain. */
  cable?: 'idle' | 'searching' | 'found' | 'none'
  progress?: number
}>()

const NOTES: Record<PictureBoard['state'], string> = {
  absent: '',
  new: 'found · no firmware yet',
  installing: 'installing the firmware…',
  old: 'firmware needs an update',
  ready: 'firmware is running',
  unplugged: 'unplugged',
}

const W = 640
const MID = W / 2
const BOARD = 128
const count = computed(() => props.boards.length)
/** How far each end of the cable has grown towards the middle, 0 to 100. */
const reach = computed(() => Math.round(Math.min(1, Math.max(0.04, props.progress ?? 0)) * 100))

interface Slot {
  id: 'single' | 'first' | 'second'
  x: number
  shown: boolean
  /** Where on the board its controller sits, and where its USB cable meets the computer. */
  port: number
  socket: number
  board: PictureBoard
  /** Which corner the tick sits on. */
  tick: number
}

const ghost: PictureBoard = { label: '', state: 'absent' }
// The same three boards are always there; only where they stand and whether
// they show changes, so a change of state is a movement.
const slots = computed<Slot[]>(() => {
  const [a, b] = props.boards
  if (count.value === 0) {
    return [
      { id: 'single', x: 44, shown: true, port: 64, socket: MID - 40, board: ghost, tick: 0 },
      { id: 'first', x: 292, shown: true, port: 102, socket: MID, board: ghost, tick: 0 },
      { id: 'second', x: 468, shown: true, port: 26, socket: MID + 40, board: ghost, tick: BOARD },
    ]
  }
  if (count.value === 1) {
    // One board is the keyboard in one piece until a second one says otherwise:
    // that picture moves to the middle, and the two halves step back. They
    // wait behind it, so that a second board unfolds from where the first stands.
    return [
      { id: 'single', x: MID - BOARD / 2, shown: true, port: 64, socket: MID, board: a, tick: 0 },
      { id: 'first', x: MID - BOARD / 2, shown: false, port: 64, socket: MID, board: ghost, tick: 0 },
      { id: 'second', x: MID - BOARD / 2, shown: false, port: 64, socket: MID, board: ghost, tick: BOARD },
    ]
  }
  return [
    { id: 'single', x: MID - 48 - BOARD, shown: false, port: 102, socket: MID - 34, board: ghost, tick: 0 },
    { id: 'first', x: MID - 48 - BOARD, shown: true, port: 102, socket: MID - 34, board: a, tick: 0 },
    { id: 'second', x: MID + 48, shown: true, port: 26, socket: MID + 34, board: b, tick: BOARD },
  ]
})
const first = computed(() => slots.value[1])
const second = computed(() => slots.value[2])
const wire = (slot: Slot) => `M${slot.x + slot.port},50 C${slot.x + slot.port},34 ${slot.socket},40 ${slot.socket},24`
const live = (slot: Slot) => slot.board.state !== 'absent' && slot.board.state !== 'unplugged'

/** The cable runs between the inner edges of the two halves. */
const span = computed(() => {
  const from = first.value.x + BOARD
  const to = second.value.x
  const middle = (from + to) / 2
  const sag = Math.min(22, (to - from) / 4)
  return {
    whole: `M${from},92 C${from + sag},${92 + sag} ${to - sag},${92 + sag} ${to},92`,
    left: `M${from},92 C${from + sag / 2},${92 + sag / 2} ${middle - sag / 2},${92 + sag * 0.75} ${middle},${92 + sag * 0.75}`,
    right: `M${to},92 C${to - sag / 2},${92 + sag / 2} ${middle + sag / 2},${92 + sag * 0.75} ${middle},${92 + sag * 0.75}`,
    middle,
    low: 92 + sag * 0.75,
  }
})
const keys = [...Array(15).keys()].map((n) => ({ x: 14 + (n % 5) * 21, y: 62 + Math.floor(n / 5) * 19 }))
const path = (d: string) => ({ d: `path('${d}')` })
</script>

<template>
  <svg class="picture" :class="['count-' + count, 'cable-' + (cable ?? 'idle')]" :viewBox="`0 0 ${W} 156`" role="img" data-test="split-picture">
    <!-- The computer and the USB cables to it only appear with a board that is really plugged in. -->
    <g class="fade" :class="{ gone: count === 0 }">
      <rect class="computer" :x="MID - 62" y="4" width="124" height="20" rx="6" />
      <text class="small" :x="MID" y="18" text-anchor="middle">this computer</text>
    </g>

    <path
      v-for="slot in slots"
      :key="'w' + slot.id"
      class="wire usb"
      :class="{ off: !live(slot), gone: !slot.shown || slot.board.state === 'absent' }"
      :style="path(wire(slot))"
    />

    <!-- The cable between the halves, and the search for it growing in from both ends. -->
    <path class="wire cable" :class="{ gone: count === 1, found: cable === 'found' && count === 2 }" :style="path(span.whole)" />
    <path class="wire reach" :class="{ gone: cable !== 'searching' }" pathLength="100" :style="{ ...path(span.left), strokeDashoffset: 100 - reach }" />
    <path class="wire reach" :class="{ gone: cable !== 'searching' }" pathLength="100" :style="{ ...path(span.right), strokeDashoffset: 100 - reach }" />
    <g class="tick fade" :class="{ gone: !(cable === 'found' && count === 2) }" :style="{ transform: `translate(${span.middle}px, ${span.low}px)` }" data-test="picture-cable-found" :data-found="cable === 'found' && count === 2">
      <circle r="9" />
      <path d="M-4,0 l3,3 l5,-6" />
    </g>
    <text class="small fade" :class="{ gone: !(cable === 'none' && count === 2) }" :x="MID" y="130" text-anchor="middle">no cable found</text>

    <g
      v-for="slot in slots"
      :key="slot.id"
      class="board"
      :class="[slot.board.state, { gone: !slot.shown }]"
      :style="{ transform: `translateX(${slot.x}px)` }"
      :data-test="'picture-board-' + slot.id"
      :data-state="slot.shown ? slot.board.state : 'hidden'"
    >
      <rect class="case" x="0" y="50" :width="BOARD" height="68" rx="10" />
      <rect class="chip" :style="{ transform: `translateX(${slot.port - 12}px)` }" x="0" y="44" width="24" height="12" rx="3" />
      <rect v-for="(key, n) in keys" :key="n" class="dot" :x="key.x" :y="key.y" width="16" height="14" rx="3" />
      <g class="tick fade" :class="{ gone: slot.board.state !== 'ready' }" :style="{ transform: `translate(${slot.tick}px, 50px)` }">
        <circle r="9" />
        <path d="M-4,0 l3,3 l5,-6" />
      </g>
      <rect class="bar fade" :class="{ gone: slot.board.state !== 'installing' }" x="14" y="122" :width="100 * Math.min(1, slot.board.progress ?? 0)" height="3" rx="1.5" />
      <text class="name" :x="BOARD / 2" y="137" text-anchor="middle">{{ slot.board.label }}</text>
      <text class="small" :x="BOARD / 2" y="150" text-anchor="middle">{{ slot.board.note ?? NOTES[slot.board.state] }}</text>
    </g>

    <!-- With nothing plugged in: what each of the two offers is. -->
    <g class="fade" :class="{ gone: count !== 0 }">
      <text class="small" x="108" y="137" text-anchor="middle">a keyboard in one piece</text>
      <text class="small" x="240" y="90" text-anchor="middle">or</text>
      <text class="small" x="444" y="137" text-anchor="middle">a split: two halves and their cable</text>
    </g>
  </svg>
</template>

<style scoped>
.picture {
  --move: 0.55s cubic-bezier(0.3, 0.8, 0.2, 1);
  width: 100%;
  max-width: 580px;
  height: auto;
  display: block;
  overflow: visible;
}

.computer {
  fill: var(--bg);
  stroke: var(--line);
}

.small {
  fill: var(--muted);
  font-size: 10px;
}

.name {
  fill: var(--text);
  font-size: 11px;
  font-weight: 600;
}

.fade,
.wire,
.board {
  transition:
    opacity 0.35s var(--ease),
    transform var(--move),
    d var(--move),
    stroke 0.3s var(--ease);
}

.gone {
  opacity: 0;
  pointer-events: none;
}

.wire {
  fill: none;
  stroke: var(--muted);
  stroke-width: 2.5;
  stroke-linecap: round;
}

.wire.off,
.wire.cable {
  stroke: var(--line);
  stroke-dasharray: 3 6;
}

.wire.cable.found {
  stroke: var(--signal);
  stroke-dasharray: none;
}

.wire.reach {
  stroke: var(--accent);
  stroke-width: 3.5;
  stroke-dasharray: 100;
  transition:
    stroke-dashoffset 0.25s linear,
    opacity 0.35s var(--ease),
    d var(--move);
}

.case {
  fill: var(--panel);
  stroke: var(--line);
  stroke-width: 1.5;
  transition: stroke 0.3s var(--ease);
}

.chip {
  fill: #3a3d42;
  stroke: #5a5e66;
  transition: transform var(--move);
}

.dot {
  fill: var(--cap);
  transition: fill 0.3s var(--ease);
}

.board.ready .case {
  stroke: var(--ok);
}

.board.ready .dot {
  fill: var(--cap-top);
}

.board.new .case,
.board.installing .case,
.board.old .case {
  stroke: var(--accent);
}

.board.absent:not(.gone),
.board.unplugged:not(.gone) {
  opacity: 0.6;
}

.board.absent .case,
.board.unplugged .case {
  stroke-dasharray: 4 5;
}

.bar {
  fill: var(--accent);
  transition:
    width 0.12s linear,
    opacity 0.3s var(--ease);
}

.tick circle {
  fill: var(--ok);
}

.tick path {
  fill: none;
  stroke: var(--accent-ink);
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}
</style>
