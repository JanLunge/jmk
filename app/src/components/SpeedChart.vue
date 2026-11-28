<script setup lang="ts">
// Speed over the seconds of a typing test: the speed of the whole run so
// far, the speed within each second behind it, and where the mistakes fell.
import { computed, ref } from 'vue'
import type { Sample } from '../lib/typingtest'

const props = defineProps<{ samples: Sample[] }>()

const W = 380
const H = 150
const LEFT = 30
const RIGHT = 10
const TOP = 10
/** Under the plot: a row for the mistakes, then the seconds. */
const BOTTOM = 34

/** The speed within a second jumps in steps of twelve; three seconds together read as a line. */
const raw = computed(() =>
  props.samples.map((_, index) => {
    const near = props.samples.slice(Math.max(0, index - 1), index + 2)
    return near.reduce((sum, each) => sum + each.raw, 0) / near.length
  }),
)
const top = computed(() => Math.max(20, Math.ceil(Math.max(...props.samples.map((each) => each.wpm), ...raw.value) / 20) * 20))
const x = (index: number) => LEFT + (props.samples.length < 2 ? 0 : (index / (props.samples.length - 1)) * (W - LEFT - RIGHT))
const y = (value: number) => TOP + (1 - value / top.value) * (H - TOP - BOTTOM)
const path = (values: number[]) => values.map((value, index) => `${index ? 'L' : 'M'}${x(index).toFixed(1)},${y(value).toFixed(1)}`).join(' ')
const lines = computed(() => [0, top.value / 2, top.value])
const seconds = computed(() => {
  const count = props.samples.length
  return [...new Set([1, Math.round(count / 2), count])].filter((second) => second >= 1)
})

const hover = ref<number>()
function point(event: PointerEvent) {
  const box = (event.currentTarget as SVGElement).getBoundingClientRect()
  const at = ((event.clientX - box.left) / box.width) * W
  const index = Math.round(((at - LEFT) / (W - LEFT - RIGHT)) * (props.samples.length - 1))
  hover.value = Math.max(0, Math.min(props.samples.length - 1, index))
}
const shown = computed(() => (hover.value === undefined ? undefined : props.samples[hover.value]))
</script>

<template>
  <figure class="chart" data-test="speed-chart">
    <figcaption class="legend">
      <span><i class="swatch wpm" />speed</span>
      <span><i class="swatch raw" />raw</span>
      <span><i class="dot" />mistake</span>
    </figcaption>
    <div class="plot">
      <svg :viewBox="`0 0 ${W} ${H}`" role="img" aria-label="Speed over the seconds of the test" @pointermove="point" @pointerleave="hover = undefined">
        <g class="grid">
          <template v-for="value in lines" :key="value">
            <line :x1="LEFT" :x2="W - RIGHT" :y1="y(value)" :y2="y(value)" />
            <text :x="LEFT - 6" :y="y(value)" text-anchor="end" dominant-baseline="central">{{ value }}</text>
          </template>
          <text v-for="second in seconds" :key="'s' + second" :x="x(second - 1)" :y="H - 4" :text-anchor="second === 1 ? 'start' : second === samples.length ? 'end' : 'middle'">{{ second }} s</text>
        </g>
        <path class="raw" :d="path(raw)" />
        <path class="wpm" :d="path(samples.map((each) => each.wpm))" />
        <template v-for="(sample, index) in samples" :key="index">
          <circle v-if="sample.errors" class="error" :cx="x(index)" :cy="H - BOTTOM + 10" :r="Math.min(5, 3 + sample.errors * 0.5)" />
        </template>
        <g v-if="hover !== undefined && shown">
          <line class="cross" :x1="x(hover)" :x2="x(hover)" :y1="TOP" :y2="H - BOTTOM + 16" />
          <circle class="mark" :cx="x(hover)" :cy="y(shown.wpm)" r="4.5" />
        </g>
      </svg>
      <div v-if="hover !== undefined && shown" class="tip card" :class="{ flip: hover > samples.length / 2 }" :style="{ left: (x(hover) / W) * 100 + '%' }">
        <strong>{{ shown.second }} s</strong>
        <span>{{ Math.round(shown.wpm) }} wpm</span>
        <span class="muted">{{ Math.round(raw[hover]) }} raw</span>
        <span v-if="shown.errors" class="muted">{{ shown.errors }} {{ shown.errors === 1 ? 'mistake' : 'mistakes' }}</span>
      </div>
    </div>
    <table class="unseen">
      <caption>Speed for each second of the test</caption>
      <thead><tr><th>Second</th><th>Words a minute</th><th>Raw</th><th>Mistakes</th></tr></thead>
      <tbody>
        <tr v-for="sample in samples" :key="sample.second"><td>{{ sample.second }}</td><td>{{ Math.round(sample.wpm) }}</td><td>{{ sample.raw }}</td><td>{{ sample.errors }}</td></tr>
      </tbody>
    </table>
  </figure>
</template>

<style scoped>
.chart {
  margin: 0;
  width: 100%;
  max-width: 420px;
  min-width: 0;
}

.legend {
  display: flex;
  gap: 14px;
  justify-content: flex-end;
  font-size: 0.74rem;
  color: var(--muted);
  padding: 0 4px 2px 0;
}

.legend span {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  white-space: nowrap;
}

.swatch {
  width: 14px;
  height: 0;
  border-top: 2px solid var(--accent);
  border-radius: 2px;
}

.swatch.raw {
  border-top: 2px dotted var(--muted);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--danger);
}

.plot {
  position: relative;
}

svg {
  display: block;
  width: 100%;
  height: auto;
  touch-action: none;
}

.grid line {
  stroke: var(--line);
  stroke-width: 1;
}

.grid text {
  fill: var(--muted);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
}

path {
  fill: none;
  stroke-linecap: round;
  stroke-linejoin: round;
}

/* Grey and dashed: told from the speed by more than its colour. */
.raw {
  stroke: var(--muted);
  stroke-width: 1.5;
  stroke-dasharray: 1 4;
}

.wpm {
  stroke: var(--accent);
  stroke-width: 2;
}

.error {
  fill: var(--danger);
  stroke: var(--bg);
  stroke-width: 1.5;
}

.cross {
  stroke: var(--muted);
  stroke-width: 1;
  stroke-dasharray: 2 3;
}

.mark {
  fill: var(--accent);
  stroke: var(--bg);
  stroke-width: 2;
}

.tip {
  position: absolute;
  top: 0;
  transform: translateX(10px);
  display: flex;
  flex-direction: column;
  padding: 6px 10px;
  border-radius: 9px;
  font-size: 0.76rem;
  line-height: 1.45;
  white-space: nowrap;
  pointer-events: none;
  box-shadow: var(--shadow);
  z-index: 5;
}

.tip.flip {
  transform: translateX(calc(-100% - 10px));
}

.unseen {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}
</style>
