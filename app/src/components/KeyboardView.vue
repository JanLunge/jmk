<script setup lang="ts">
// Draws a keyboard from key shapes. Used everywhere a keyboard is shown:
// the keymap, the layout editor, the tester and the virtual boards.
import { computed, ref } from 'vue'
import { carriesKey } from '../lib/drag'
import type { Legend } from '../lib/keycodes'
import { bounds, hasKey, knobKey } from '../lib/layout'
import type { KeyGeometry } from '../lib/protocol'

const props = withDefaults(
  defineProps<{
    layout: KeyGeometry[]
    /** What to print on the key at a position. */
    legend?: (pos: number) => Legend | undefined
    pressed?: Set<number>
    /** Held keys whose action on the keyboard is not the one drawn here. */
    pressedElsewhere?: Set<number>
    selected?: number[]
    /** Positions drawn faded (e.g. keys not yet found). */
    dim?: number[]
    interactive?: boolean
    /** Things dragged from the picker can be dropped on the keys. */
    droppable?: boolean
    /** A colour per position, for keys that lead to a coloured layer. */
    tint?: (pos: number) => string | undefined
    /**
     * Positions drawn as a knob instead of a key. Each is the first of a
     * knob's two positions: turning left, then (at the next position) right.
     */
    knobs?: number[]
    /** RGB lights to draw, in layout units. `under` lights sit beneath the keys and glow out. */
    lights?: { x: number; y: number; color: string; under: boolean; marked?: boolean }[]
    /** Largest key size in pixels; the drawing shrinks to fit its box. */
    maxUnit?: number
  }>(),
  { maxUnit: 62 },
)

const emit = defineEmits<{
  down: [pos: number, event: PointerEvent]
  up: [pos: number, event: PointerEvent]
  drop: [pos: number, event: DragEvent]
}>()

/** The key something dragged is hovering over. */
const dropAt = ref<number>()
function over(pos: number, event: DragEvent) {
  if (!props.droppable || !carriesKey(event)) return
  event.preventDefault()
  dropAt.value = pos
}
function land(pos: number, event: DragEvent) {
  if (!props.droppable) return
  event.preventDefault()
  dropAt.value = undefined
  emit('drop', pos, event)
}

const PAD = 14
const box = computed(() => bounds(props.layout))
const viewBox = computed(() => `${box.value.x - PAD} ${box.value.y - PAD} ${box.value.w + 2 * PAD} ${box.value.h + 2 * PAD}`)
const maxWidth = computed(() => ((box.value.w + 2 * PAD) / 100) * props.maxUnit)

const isKnob = (pos: number) => !!props.knobs?.includes(pos)
/** Per knob in the picture, the switch it sits on (-1: none). */
const presses = computed(() => new Map((props.knobs ?? []).map((pos) => [pos, knobKey(props.layout, props.knobs!, pos)])))
const underKnob = computed(() => new Set(presses.value.values()))

const keys = computed(() =>
  props.layout
    .map((key, pos) => ({ key, pos }))
    .filter(({ key, pos }) => hasKey(key) && !isKnob(pos) && !underKnob.value.has(pos))
    .map(({ key, pos }) => {
      const legend = props.legend?.(pos)
      return {
        pos,
        key,
        legend,
        tint: props.tint?.(pos),
        transform: `rotate(${key.r / 10} ${key.x + key.w / 2} ${key.y + key.h / 2})`,
        fontSize: fontSize(legend?.main ?? '', key.w),
      }
    }),
)

const dials = computed(() =>
  (props.knobs ?? [])
    .filter((pos) => hasKey(props.layout[pos]))
    .map((pos) => {
      const key = props.layout[pos]
      const cx = key.x + key.w / 2
      const cy = key.y + key.h / 2
      const r = Math.min(key.w, key.h) / 2 - 5
      const press = presses.value.get(pos)!
      const open = [pos, pos + 1, press].some((part) => props.selected?.includes(part))
      const [tw, th, top] = open ? [96, 46, cy - r - 36] : [r + 2, 24, cy - r - 4]
      return {
        pos,
        cx,
        cy,
        r,
        transform: `rotate(${key.r / 10} ${cx} ${cy})`,
        // Left turns one way, right the other. A knob on a switch is that key, round, with the two turns as tabs
        // on its top edge; they grow while one of the three is selected, to be easy to hit.
        halves: (press < 0
          ? [
              { pos, d: `M ${cx} ${cy - r} A ${r} ${r} 0 0 0 ${cx} ${cy + r} Z`, x: cx - r / 2, y: cy + 6, arrow: '⟲', tab: '', room: r - 8, most: 18 },
              { pos: pos + 1, d: `M ${cx} ${cy - r} A ${r} ${r} 0 0 1 ${cx} ${cy + r} Z`, x: cx + r / 2, y: cy + 6, arrow: '⟳', tab: '', room: r - 8, most: 18 },
            ]
          : [
              { pos: press, d: `M ${cx - r} ${cy} a ${r} ${r} 0 1 0 ${2 * r} 0 a ${r} ${r} 0 1 0 ${-2 * r} 0 Z`, x: cx, y: cy + 14, arrow: '', tab: '', room: 1.6 * r, most: 30 },
              ...[
                { pos, x: cx - tw - 1, tab: '⟲ ' },
                { pos: pos + 1, x: cx + 1, tab: '⟳ ' },
              ].map((turn) => ({ ...turn, d: pill(turn.x, top, tw, th), x: turn.x + tw / 2, y: top + th / 2 + 4, arrow: '', room: tw - 10, most: open ? 22 : 13 })),
            ]
        ).map((half) => ({ ...half, text: half.tab + (props.legend?.(half.pos)?.main ?? '') })),
        open,
      }
    })
    // The one with its tabs grown goes on top.
    .sort((a, b) => Number(a.open) - Number(b.open)),
)

const pill = (x: number, y: number, w: number, h: number) => `M ${x + h / 2} ${y} h ${w - h} a ${h / 2} ${h / 2} 0 0 1 0 ${h} h ${h - w} a ${h / 2} ${h / 2} 0 0 1 0 ${-h} Z`

function fontSize(text: string, width: number): number {
  const fit = (width - 22) / Math.max(text.length, 1) / 0.6
  return Math.max(13, Math.min(34, fit))
}
</script>

<template>
  <svg class="keyboard" :viewBox="viewBox" :style="{ maxWidth: maxWidth + 'px' }" role="img">
    <defs>
      <filter id="jmk-glow" x="-150%" y="-150%" width="400%" height="400%">
        <feGaussianBlur stdDeviation="16" />
      </filter>
    </defs>
    <g v-if="lights" class="glows">
      <template v-for="(light, index) in lights" :key="index">
        <circle v-if="light.under && light.color !== '#000000'" :cx="light.x" :cy="light.y" r="46" :fill="light.color" filter="url(#jmk-glow)" />
      </template>
    </g>
    <g
      v-for="{ pos, key, legend, tint, transform, fontSize } in keys"
      :key="pos"
      class="key"
      :class="[
        legend ? 'kind-' + legend.kind : 'kind-key',
        {
          pressed: pressed?.has(pos),
          elsewhere: pressedElsewhere?.has(pos),
          selected: selected?.includes(pos),
          dim: dim?.includes(pos),
          interactive,
          tinted: !!tint,
          target: dropAt === pos,
        },
      ]"
      :style="tint ? { '--tint': tint } : undefined"
      :transform="transform"
      :data-pos="pos"
      @dragover="over(pos, $event)"
      @dragleave="dropAt === pos && (dropAt = undefined)"
      @drop="land(pos, $event)"
      @pointerdown="interactive && emit('down', pos, $event)"
      @pointerup="interactive && emit('up', pos, $event)"
    >
      <g class="cap">
        <rect class="base" :x="key.x + 3" :y="key.y + 3" :width="key.w - 6" :height="key.h - 6" rx="13" />
        <rect class="top" :x="key.x + 9" :y="key.y + 6" :width="key.w - 18" :height="key.h - 20" rx="9" />
        <text
          v-if="legend?.main"
          class="main"
          :x="key.x + key.w / 2"
          :y="key.y + (legend.sub ? key.h / 2 - 8 : key.h / 2 - 3)"
          :font-size="fontSize"
        >
          {{ legend.main }}
        </text>
        <text v-if="legend?.sub" class="sub" :x="key.x + key.w / 2" :y="key.y + key.h / 2 + 19">{{ legend.sub }}</text>
      </g>
    </g>
    <g v-for="dial in dials" :key="'knob' + dial.pos" class="dial" :class="{ interactive }" :transform="dial.transform" :data-knob="dial.pos">
      <circle class="rim" :cx="dial.cx" :cy="dial.cy" :r="dial.r + 3" />
      <g
        v-for="half in dial.halves"
        :key="half.pos"
        class="half"
        :class="{ selected: selected?.includes(half.pos), pressed: pressed?.has(half.pos), target: dropAt === half.pos }"
        :data-pos="half.pos"
        @dragover="over(half.pos, $event)"
        @dragleave="dropAt === half.pos && (dropAt = undefined)"
        @drop="land(half.pos, $event)"
        @pointerdown="interactive && emit('down', half.pos, $event)"
        @pointerup="interactive && emit('up', half.pos, $event)"
      >
        <path :d="half.d" />
        <text class="arrow" :x="half.x" :y="half.y - dial.r / 2.4 - 6">{{ half.arrow }}</text>
        <text class="main" :x="half.x" :y="half.y" :font-size="Math.max(half.tab ? 9 : 11, Math.min(half.most, half.room / Math.max(half.text.length, 1) / 0.6))">
          {{ half.text }}
        </text>
      </g>
    </g>
    <g v-if="lights" class="lights">
      <circle
        v-for="(light, index) in lights"
        :key="index"
        class="light"
        :class="{ under: light.under, marked: light.marked }"
        :cx="light.x"
        :cy="light.y"
        :r="light.marked ? 13 : 8"
        :fill="light.color"
        :data-light="index"
      />
    </g>
  </svg>
</template>

<style scoped>
.keyboard {
  display: block;
  /* Underglow shines past the edge of the picture. */
  overflow: visible;
  width: 100%;
  height: auto;
  user-select: none;
  touch-action: none;
}

.cap {
  transition: transform 0.09s var(--ease);
  transform-box: fill-box;
  transform-origin: center;
}

.base {
  fill: var(--cap);
  stroke: transparent;
  stroke-width: 3;
  transition:
    stroke 0.15s,
    fill 0.15s;
}

.top {
  fill: var(--cap-top);
  transition: fill 0.12s;
}

text {
  fill: var(--cap-text);
  text-anchor: middle;
  dominant-baseline: central;
  pointer-events: none;
  font-weight: 600;
}

.sub {
  font-size: 12px;
  font-weight: 500;
  opacity: 0.6;
}

.interactive {
  cursor: pointer;
}

.interactive:hover .base {
  stroke: var(--muted);
}

.selected .base {
  stroke: var(--accent);
}

.pressed .cap {
  transform: translateY(3px) scale(0.97);
}

.pressed .top {
  fill: var(--accent);
}

.pressed text {
  fill: var(--accent-ink);
}

.dim {
  opacity: 0.3;
}

.rim {
  fill: var(--cap);
}

.half path {
  fill: var(--cap-top);
  stroke: var(--cap);
  stroke-width: 2;
  transition: fill 0.12s;
}

.half .arrow {
  font-size: 15px;
  opacity: 0.55;
}

.dial.interactive .half {
  cursor: pointer;
}

.dial.interactive .half:hover path {
  stroke: var(--muted);
}

.half.selected path {
  stroke: var(--accent);
  stroke-width: 3;
}

.half.pressed path {
  fill: var(--accent);
}

.light {
  stroke: color-mix(in srgb, var(--text) 55%, transparent);
  stroke-width: 1.5;
  pointer-events: none;
}

.light.under {
  stroke-dasharray: 3 3;
}

.light.marked {
  stroke: var(--accent);
  stroke-width: 4;
}

/* Held, but doing something other than what is printed on it here. */
.elsewhere .cap {
  transform: translateY(1.5px);
}

.elsewhere .base {
  stroke: color-mix(in srgb, var(--accent) 45%, transparent);
  stroke-dasharray: 4 5;
}

.elsewhere .top {
  fill: color-mix(in srgb, var(--accent) 14%, var(--cap-top));
}

.kind-mod .top {
  fill: color-mix(in srgb, #6ea8ff 28%, var(--cap-top));
}

.kind-layer .top {
  fill: color-mix(in srgb, var(--accent) 34%, var(--cap-top));
}

.tinted .top {
  fill: color-mix(in srgb, var(--tint) 42%, var(--cap-top));
}

.tinted .base {
  fill: color-mix(in srgb, var(--tint) 55%, var(--cap));
}

.kind-media .top {
  fill: color-mix(in srgb, #c58bff 30%, var(--cap-top));
}

.kind-macro .top {
  fill: color-mix(in srgb, var(--ok) 28%, var(--cap-top));
}

.kind-special .top {
  fill: color-mix(in srgb, var(--danger) 24%, var(--cap-top));
}

.kind-through .top,
.kind-empty .top {
  fill: transparent;
}

.kind-through .base,
.kind-empty .base {
  fill: transparent;
  stroke: var(--line);
  stroke-dasharray: 6 6;
}

.kind-through text {
  opacity: 0.35;
}

/* A see-through key has only a dashed outline, so being pointed at or selected has to show on it as well. */
.kind-through.interactive:hover .base,
.kind-empty.interactive:hover .base {
  stroke: var(--muted);
  stroke-dasharray: none;
}

.kind-through.selected .base,
.kind-empty.selected .base {
  stroke: var(--accent);
  stroke-dasharray: none;
  fill: color-mix(in srgb, var(--accent) 14%, transparent);
}

.kind-through.selected text {
  opacity: 0.75;
}
.key.pressed .top {
  fill: var(--accent);
}

.target .base,
.half.target path {
  stroke: var(--accent);
  stroke-dasharray: 5 4;
}
</style>
