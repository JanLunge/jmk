<script setup lang="ts">
// A slim field in the header to type into, for trying the keyboard out. It
// takes what the computer makes of the keys; the keys that arrived last are
// named beside the text, so keys that print nothing (Esc, arrows, F13) show too.
import { ref } from 'vue'

const text = ref('')
const field = ref<HTMLInputElement>()
const focused = ref(false)
const recent = ref<{ id: number; label: string }[]>([])
let ids = 0

const NAMES: Record<string, string> = {
  ' ': 'Space', Escape: 'Esc', Backspace: 'Bksp', Delete: 'Del', ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→',
  Control: 'Ctrl', Meta: 'GUI', AudioVolumeUp: 'Vol+', AudioVolumeDown: 'Vol−', AudioVolumeMute: 'Mute',
  MediaPlayPause: 'Play', MediaTrackNext: 'Next', MediaTrackPrevious: 'Prev', MediaStop: 'Stop',
}

function onKey(event: KeyboardEvent) {
  // Tab and Enter would walk off or submit; here they are keys like any other.
  if (event.key === 'Tab' || event.key === 'Enter') event.preventDefault()
  if (event.repeat) return
  const key = NAMES[event.key] ?? (event.key === 'Unidentified' || event.key === 'Dead' ? event.code : event.key)
  const mods = [event.ctrlKey && key !== 'Ctrl' && 'Ctrl', event.altKey && key !== 'Alt' && 'Alt', event.metaKey && key !== 'GUI' && 'GUI']
  recent.value = [...recent.value, { id: ++ids, label: [...mods.filter(Boolean), key].join(' + ') }].slice(-4)
}

function clear() {
  text.value = ''
  recent.value = []
  field.value?.focus()
}
</script>

<template>
  <div class="try" :class="{ open: focused || !!text || recent.length > 0 }" data-test="scratch" @click="field?.focus()">
    <input
      ref="field"
      v-model="text"
      class="field mono"
      placeholder="Type here to try it"
      spellcheck="false"
      autocomplete="off"
      autocapitalize="off"
      data-test="scratch-text"
      @keydown="onKey"
      @focus="focused = true"
      @blur="focused = false"
    />
    <TransitionGroup name="pop" tag="div" class="keys" data-test="scratch-keys">
      <span v-for="key in recent" :key="key.id" class="key">{{ key.label }}</span>
    </TransitionGroup>
    <button v-if="text || recent.length" class="clear" title="Clear" data-test="scratch-clear" @click.stop="clear">×</button>
  </div>
</template>

<style scoped>
.try {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 190px;
  height: 38px;
  padding: 0 6px 0 12px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: var(--bg);
  cursor: text;
  transition:
    width 0.25s var(--ease),
    border-color 0.15s,
    box-shadow 0.15s;
}

.try.open {
  width: min(460px, 42vw);
}

.try:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-soft);
}

.field {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: none;
  color: var(--text);
  font-size: 0.95rem;
}

.keys {
  display: flex;
  gap: 4px;
  flex: none;
  max-width: 60%;
  overflow: hidden;
  justify-content: flex-end;
}

.key {
  padding: 1px 7px;
  border-radius: 6px;
  background: var(--cap-top);
  border-bottom: 2px solid var(--cap);
  font-size: 0.78rem;
  font-weight: 600;
  white-space: nowrap;
}

.key:last-child {
  background: var(--accent);
  color: var(--accent-ink);
}

.clear {
  flex: none;
  border: none;
  background: none;
  color: var(--muted);
  font-size: 1rem;
  cursor: pointer;
  padding: 2px 6px;
}

.clear:hover {
  color: var(--text);
}
</style>
