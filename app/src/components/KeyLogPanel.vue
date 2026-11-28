<script setup lang="ts">
// The timing log as a list: every key press with how long after the one
// before it came and how long it was held. Newest first.
import { computed } from 'vue'
import { formatMs, type KeyLog } from '../lib/keylog'

const props = defineProps<{ log: KeyLog }>()
defineEmits<{ clear: []; close: [] }>()

const rows = computed(() => [...props.log.entries].reverse())
</script>

<template>
  <div class="log card" data-test="scratch-log" role="dialog" aria-label="Timing log">
    <div class="row head">
      <strong>Timing log</strong>
      <span class="muted small">{{ log.entries.length ? `${log.entries.length} ${log.entries.length === 1 ? 'key' : 'keys'}, newest first` : '' }}</span>
      <span class="grow" />
      <button class="btn small" :disabled="!log.entries.length" data-test="scratch-log-clear" @click="$emit('clear')">Clear</button>
      <button class="close" title="Close (Esc)" aria-label="Close the timing log" data-test="scratch-log-close" @click="$emit('close')">×</button>
    </div>
    <p v-if="!log.entries.length" class="muted small empty">Nothing yet. Every key pressed shows here with its timings, the keys that print nothing too.</p>
    <template v-else>
      <div class="line labels muted">
        <span>Key</span>
        <span title="From the press before this one to this press">After previous</span>
        <span title="From its press to its release">Held</span>
      </div>
      <ol class="rows scroll">
        <li v-for="entry in rows" :key="entry.id" class="line" data-test="scratch-log-row">
          <span class="name"><span class="cap">{{ entry.label }}</span></span>
          <span class="mono num" data-test="log-gap">{{ entry.gap === undefined ? 'first' : formatMs(entry.gap) }}</span>
          <span class="mono num" :class="{ live: entry.down }" data-test="log-held">{{ entry.down ? 'down' : formatMs(entry.held) }}</span>
        </li>
      </ol>
    </template>
  </div>
</template>

<style scoped>
.log {
  width: min(340px, calc(100vw - 100px));
  padding: 12px 6px 8px 14px;
  box-shadow: var(--shadow);
  cursor: default;
}

.head {
  padding-right: 8px;
  margin-bottom: 8px;
}

.close {
  width: 26px;
  height: 26px;
  margin-left: -4px;
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

.empty {
  padding: 4px 8px 8px 0;
}

.line {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 96px 78px;
  align-items: center;
  gap: 8px;
  padding: 3px 0;
}

.labels {
  padding-right: 14px;
  font-size: 0.74rem;
  border-bottom: 1px solid var(--line);
}

.labels span:not(:first-child),
.num {
  text-align: right;
}

.rows {
  list-style: none;
  margin: 0;
  padding: 2px 8px 0 0;
  max-height: min(320px, 48vh);
  overflow-y: scroll;
}

.rows .line + .line {
  border-top: 1px solid color-mix(in srgb, var(--line) 55%, transparent);
}

.name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cap {
  padding: 1px 7px;
  border-radius: 6px;
  background: var(--cap-top);
  border-bottom: 2px solid var(--cap);
  font-size: 0.78rem;
  font-weight: 600;
}

.num {
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.num.live {
  color: var(--accent);
}
</style>
