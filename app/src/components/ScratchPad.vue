<script setup lang="ts">
// A place to type into, for trying the keyboard out. The text field takes
// what the computer makes of the keys; the strip beside it names every key
// that arrived, so keys that print nothing (Esc, arrows, F13) show up too.
import { nextTick, onMounted, ref } from 'vue'

defineEmits<{ close: [] }>()

const text = ref('')
const field = ref<HTMLTextAreaElement>()
const recent = ref<{ id: number; label: string }[]>([])
let ids = 0

const NAMES: Record<string, string> = {
  ' ': 'Space', Escape: 'Esc', Backspace: 'Bksp', Delete: 'Del', ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→',
  Control: 'Ctrl', Meta: 'GUI', AudioVolumeUp: 'Vol+', AudioVolumeDown: 'Vol−', AudioVolumeMute: 'Mute',
  MediaPlayPause: 'Play', MediaTrackNext: 'Next', MediaTrackPrevious: 'Prev', MediaStop: 'Stop',
}

function onKey(event: KeyboardEvent) {
  // Tab would walk off to the next button; here it is a key like any other.
  if (event.key === 'Tab') event.preventDefault()
  if (event.repeat) return
  const key = NAMES[event.key] ?? (event.key === 'Unidentified' || event.key === 'Dead' ? event.code : event.key)
  const mods = [event.ctrlKey && key !== 'Ctrl' && 'Ctrl', event.altKey && key !== 'Alt' && 'Alt', event.metaKey && key !== 'GUI' && 'GUI']
  recent.value = [...recent.value, { id: ++ids, label: [...mods.filter(Boolean), key].join(' + ') }].slice(-10)
}

function clear() {
  text.value = ''
  recent.value = []
  field.value?.focus()
}

onMounted(() => nextTick(() => field.value?.focus()))
</script>

<template>
  <section class="pad" data-test="scratch">
    <textarea
      ref="field"
      v-model="text"
      class="field mono"
      placeholder="Type here to try your keyboard…"
      spellcheck="false"
      autocomplete="off"
      autocapitalize="off"
      data-test="scratch-text"
      @keydown="onKey"
    />
    <div class="side col">
      <div class="row head">
        <span class="muted small">Keys that arrived</span>
        <span class="grow" />
        <button class="btn small ghost" data-test="scratch-clear" @click="clear">Clear</button>
        <button class="btn small ghost" title="Close" data-test="scratch-close" @click="$emit('close')">✕</button>
      </div>
      <TransitionGroup name="pop" tag="div" class="keys" data-test="scratch-keys">
        <span v-for="key in recent" :key="key.id" class="key">{{ key.label }}</span>
      </TransitionGroup>
      <span v-if="!recent.length" class="muted small">Keys without a letter, like Esc or F13, show up here.</span>
    </div>
  </section>
</template>

<style scoped>
.pad {
  flex: none;
  display: grid;
  grid-template-columns: minmax(0, 1.4fr) minmax(220px, 1fr);
  gap: 16px;
  height: 132px;
  padding: 12px 24px;
  border-top: 1px solid var(--line);
  background: var(--panel);
}

.field {
  resize: none;
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: var(--bg);
  color: var(--text);
  font-size: 1rem;
  line-height: 1.5;
  outline: none;
  transition:
    border-color 0.15s,
    box-shadow 0.15s;
}

.field:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-soft);
}

.side {
  gap: 6px;
  min-width: 0;
}

.head {
  gap: 4px;
}

.small {
  font-size: 0.85rem;
}

.keys {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
  overflow: hidden;
  max-height: 62px;
}

.key {
  padding: 2px 8px;
  border-radius: 7px;
  background: var(--cap-top);
  border-bottom: 2px solid var(--cap);
  font-size: 0.82rem;
  font-weight: 600;
  white-space: nowrap;
}

.key:last-child {
  background: var(--accent);
  color: var(--accent-ink);
}
</style>
