<script setup lang="ts">
// Draws a keyboard from key shapes. Used everywhere a keyboard is shown:
// the keymap, the layout editor, the tester and the virtual boards.
import { computed } from 'vue'
import type { Legend } from '../lib/keycodes'
import { bounds, hasKey } from '../lib/layout'
import type { KeyGeometry } from '../lib/protocol'

const props = withDefaults(
  defineProps<{
    layout: KeyGeometry[]
    /** What to print on the key at a position. */
    legend?: (pos: number) => Legend | undefined
    pressed?: Set<number>
    selected?: number[]
    /** Positions drawn faded (e.g. keys not yet found). */
    dim?: number[]
    interactive?: boolean
    /** Largest key size in pixels; the drawing shrinks to fit its box. */
    maxUnit?: number
  }>(),
  { maxUnit: 62 },
)

const emit = defineEmits<{
  down: [pos: number, event: PointerEvent]
  up: [pos: number, event: PointerEvent]
}>()

const PAD = 14
const box = computed(() => bounds(props.layout))
const viewBox = computed(() => `${box.value.x - PAD} ${box.value.y - PAD} ${box.value.w + 2 * PAD} ${box.value.h + 2 * PAD}`)
const maxWidth = computed(() => ((box.value.w + 2 * PAD) / 100) * props.maxUnit)

const keys = computed(() =>
  props.layout
    .map((key, pos) => ({ key, pos }))
    .filter(({ key }) => hasKey(key))
    .map(({ key, pos }) => {
      const legend = props.legend?.(pos)
      return {
        pos,
        key,
        legend,
        transform: `rotate(${key.r / 10} ${key.x + key.w / 2} ${key.y + key.h / 2})`,
        fontSize: fontSize(legend?.main ?? '', key.w),
      }
    }),
)

function fontSize(text: string, width: number): number {
  const fit = (width - 22) / Math.max(text.length, 1) / 0.6
  return Math.max(13, Math.min(34, fit))
}
</script>

<template>
  <svg class="keyboard" :viewBox="viewBox" :style="{ maxWidth: maxWidth + 'px' }" role="img">
    <g
      v-for="{ pos, key, legend, transform, fontSize } in keys"
      :key="pos"
      class="key"
      :class="[
        legend ? 'kind-' + legend.kind : 'kind-key',
        {
          pressed: pressed?.has(pos),
          selected: selected?.includes(pos),
          dim: dim?.includes(pos),
          interactive,
        },
      ]"
      :transform="transform"
      :data-pos="pos"
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
  </svg>
</template>

<style scoped>
.keyboard {
  display: block;
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
  fill: var(--signal);
}

.pressed text {
  fill: #06201d;
}

.dim {
  opacity: 0.3;
}

.kind-mod .top {
  fill: color-mix(in srgb, #6ea8ff 28%, var(--cap-top));
}

.kind-layer .top {
  fill: color-mix(in srgb, var(--accent) 34%, var(--cap-top));
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
</style>
