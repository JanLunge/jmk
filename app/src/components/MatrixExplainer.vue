<script setup lang="ts">
// A slowed-down picture of how a keyboard finds pressed keys: the chip pulls
// one line low at a time and looks at which of the other lines follow.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

const props = withDefaults(
  defineProps<{
    /** Names of the lines the chip switches, and of the lines it reads. */
    driven?: string[]
    sensed?: string[]
    /** Show what happens without diodes. */
    diodes?: boolean
    /** Keys held down at the start, as "driven,sensed" index pairs. */
    held?: string[]
  }>(),
  { driven: () => ['Row 1', 'Row 2'], sensed: () => ['Col 1', 'Col 2', 'Col 3'], diodes: true, held: () => ['0,1'] },
)

const CELL = 74
const LEFT = 86
const TOP = 62
const closed = ref(new Set(props.held))
const step = ref(0)
let timer: ReturnType<typeof setInterval>

onMounted(() => (timer = setInterval(() => (step.value = (step.value + 1) % props.driven.length), 900)))
onBeforeUnmount(() => clearInterval(timer))

const id = (d: number, s: number) => `${d},${s}`
function toggle(d: number, s: number) {
  const key = id(d, s)
  if (closed.value.has(key)) closed.value.delete(key)
  else closed.value.add(key)
}

/**
 * Which driven lines are pulled low while line `active` is switched on.
 * With diodes only that one; without, current sneaks back through any
 * closed switches, which is how "ghost" key presses happen.
 */
function lowLines(active: number): { driven: Set<number>; sensed: Set<number> } {
  const driven = new Set([active])
  const sensed = new Set<number>()
  let grew = true
  while (grew) {
    grew = false
    for (const key of closed.value) {
      const [d, s] = key.split(',').map(Number)
      if (driven.has(d) && !sensed.has(s)) {
        sensed.add(s)
        grew = true
      }
      if (!props.diodes && sensed.has(s) && !driven.has(d)) {
        driven.add(d)
        grew = true
      }
    }
  }
  return { driven, sensed }
}

const low = computed(() => lowLines(step.value))

/** Keys the chip believes are pressed, including ghosts. */
const seen = computed(() => {
  const out = new Set<string>()
  props.driven.forEach((_, d) => lowLines(d).sensed.forEach((s) => out.add(id(d, s))))
  return out
})

const ghosts = computed(() => [...seen.value].filter((key) => !closed.value.has(key)))
const width = computed(() => LEFT + props.sensed.length * CELL + 30)
const height = computed(() => TOP + props.driven.length * CELL + 20)
</script>

<template>
  <div class="explainer col">
    <svg :viewBox="`0 0 ${width} ${height}`" :style="{ maxWidth: width * 1.25 + 'px' }">
      <!-- lines the chip reads -->
      <g v-for="(name, s) in sensed" :key="'s' + s" :class="{ live: low.sensed.has(s) }">
        <line class="wire" :x1="LEFT + s * CELL + 52" :y1="24" :x2="LEFT + s * CELL + 52" :y2="height - 14" />
        <text class="name" :x="LEFT + s * CELL + 52" y="12" text-anchor="middle">{{ name }}</text>
      </g>
      <!-- lines the chip switches -->
      <g v-for="(name, d) in driven" :key="'d' + d" :class="{ live: low.driven.has(d), now: step === d }">
        <line class="wire" :x1="LEFT - 8" :y1="TOP + d * CELL + 44" :x2="width - 16" :y2="TOP + d * CELL + 44" />
        <text class="name" :x="LEFT - 16" :y="TOP + d * CELL + 44" text-anchor="end">{{ name }}</text>
        <circle class="scan" :cx="LEFT - 8" :cy="TOP + d * CELL + 44" r="5" />
      </g>
      <!-- switches -->
      <g v-for="(_, d) in driven" :key="'row' + d">
        <g
          v-for="(__, s) in sensed"
          :key="id(d, s)"
          class="switch"
          :class="{
            closed: closed.has(id(d, s)),
            seen: seen.has(id(d, s)),
            ghost: ghosts.includes(id(d, s)),
            flowing: closed.has(id(d, s)) && low.driven.has(d) && low.sensed.has(s),
          }"
          :transform="`translate(${LEFT + s * CELL}, ${TOP + d * CELL})`"
          :data-test="'switch-' + id(d, s)"
          @click="toggle(d, s)"
        >
          <rect class="cap" x="4" y="-6" width="36" height="36" rx="8" />
          <!-- from the read line, through the switch (and diode), to the switched line -->
          <path class="lead" d="M52 6 H22 V16" />
          <path v-if="diodes" class="lead" d="M22 30 V44" />
          <path v-else class="lead" d="M22 16 V44" />
          <g v-if="diodes" class="diode">
            <path d="M15 17 H29 L22 28 Z" />
            <line x1="15" y1="29" x2="29" y2="29" />
          </g>
          <circle class="joint" cx="52" cy="6" r="3" />
          <circle class="joint" cx="22" cy="44" r="3" />
        </g>
      </g>
    </svg>
    <p class="caption muted">
      <template v-if="ghosts.length">
        <strong class="warn">A ghost appeared.</strong> Without diodes, current sneaks backwards through the other
        pressed keys, and the chip sees a key nobody is touching.
      </template>
      <template v-else>
        The chip switches on <strong>{{ driven[step] }}</strong> and checks which lines answer.
        {{ low.sensed.size ? 'A pressed key lets the signal through.' : 'Nothing answers, so no key in it is pressed.' }}
        Click the keys to press them.
      </template>
    </p>
  </div>
</template>

<style scoped>
.explainer {
  align-items: center;
  gap: 14px;
}

svg {
  width: 100%;
  height: auto;
  user-select: none;
}

.wire {
  stroke: var(--line);
  stroke-width: 3;
  stroke-linecap: round;
  transition: stroke 0.25s;
}

.live .wire {
  stroke: var(--signal);
}

.name {
  fill: var(--muted);
  font-size: 11px;
  dominant-baseline: central;
  transition: fill 0.25s;
}

.now .name,
.live .name {
  fill: var(--text);
}

.scan {
  fill: var(--line);
  transition: fill 0.25s;
}

.now .scan {
  fill: var(--accent);
}

.switch {
  cursor: pointer;
}

.cap {
  fill: var(--cap);
  stroke: transparent;
  stroke-width: 2;
  transition:
    fill 0.2s,
    transform 0.12s var(--ease),
    stroke 0.2s;
  transform-box: fill-box;
  transform-origin: center;
}

.switch:hover .cap {
  stroke: var(--muted);
}

.closed .cap {
  fill: var(--accent);
  transform: scale(0.9);
}

.seen .cap {
  stroke: var(--signal);
}

.ghost .cap {
  fill: color-mix(in srgb, var(--danger) 45%, var(--cap));
  stroke: var(--danger);
  stroke-dasharray: 4 3;
}

.lead {
  fill: none;
  stroke: var(--line);
  stroke-width: 2;
  transition: stroke 0.25s;
}

.diode path {
  fill: var(--muted);
}

.diode line {
  stroke: var(--muted);
  stroke-width: 2.5;
}

.joint {
  fill: var(--line);
  transition: fill 0.25s;
}

.flowing .lead,
.flowing .diode line {
  stroke: var(--signal);
}

.flowing .diode path,
.flowing .joint {
  fill: var(--signal);
}

.caption {
  max-width: 440px;
  text-align: center;
  min-height: 4.5em;
}

.warn {
  color: var(--danger);
}
</style>
