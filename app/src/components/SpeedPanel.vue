<script setup lang="ts">
// How fast the typing in the field was, from a few sides: the burst typed
// last, the best one, all of them together, and the whole time with its pauses.
import { computed } from 'vue'
import { PAUSE_MS, speedStats, type Speed } from '../lib/speed'

const props = defineProps<{ speed: Speed }>()
defineEmits<{ close: [] }>()

const stats = computed(() => speedStats(props.speed))
const shown = (wpm: number | undefined) => (wpm === undefined ? '–' : String(Math.round(wpm)))
const seconds = computed(() => (stats.value.typingMs / 1000).toFixed(stats.value.typingMs < 10_000 ? 1 : 0))
const rows = computed(() => [
  { id: 'last', name: 'Last burst', hint: 'The stretch typed last without a pause', wpm: stats.value.last },
  { id: 'best', name: 'Best burst', hint: 'The fastest stretch', wpm: stats.value.best },
  { id: 'typing', name: 'While typing', hint: 'All stretches together, the pauses between them left out', wpm: stats.value.typing },
  { id: 'overall', name: 'Start to end', hint: 'From the first character to the last, pauses and all', wpm: stats.value.overall },
])
</script>

<template>
  <div class="speed card" data-test="scratch-speed-panel" role="dialog" aria-label="Typing speed">
    <div class="row head">
      <strong>Typing speed</strong>
      <span class="muted small">words per minute</span>
      <span class="grow" />
      <button class="close" title="Close (Esc)" aria-label="Close" @click="$emit('close')">×</button>
    </div>
    <dl>
      <div v-for="row in rows" :key="row.id" class="line" :title="row.hint">
        <dt>{{ row.name }}</dt>
        <dd class="mono" :data-test="'speed-' + row.id">{{ shown(row.wpm) }}</dd>
      </div>
    </dl>
    <p class="muted small note">
      {{ stats.chars }} {{ stats.chars === 1 ? 'character' : 'characters' }}, {{ stats.bursts }} {{ stats.bursts === 1 ? 'burst' : 'bursts' }}, {{ seconds }} s of typing.
      A pause of {{ PAUSE_MS / 1000 }} s ends a burst. Every character counts, taken back or not.
    </p>
  </div>
</template>

<style scoped>
.speed {
  width: min(280px, calc(100vw - 100px));
  padding: 12px 14px 10px;
  box-shadow: var(--shadow);
  cursor: default;
}

.head {
  gap: 8px;
  margin-bottom: 6px;
}

.close {
  width: 26px;
  height: 26px;
  margin-right: -6px;
  border: 0;
  border-radius: 7px;
  background: none;
  color: var(--muted);
  font-size: 1.05rem;
  line-height: 1;
  cursor: pointer;
}

.close:hover {
  color: var(--text);
  background: var(--panel-2);
}

.small {
  font-size: 0.8rem;
}

dl {
  margin: 0;
}

.line {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  padding: 5px 0;
}

.line + .line {
  border-top: 1px solid color-mix(in srgb, var(--line) 55%, transparent);
}

dd {
  margin: 0;
  font-size: 1.1rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.line:first-child dd {
  color: var(--accent);
}

.note {
  margin: 8px 0 0;
  line-height: 1.4;
}
</style>
