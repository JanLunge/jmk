<script setup lang="ts">
// A slim field in the header to type into, for trying the keyboard out. The
// keys that arrived last are named on a line of their own under the text, so
// keys that print nothing (Esc, arrows, Bksp) show too and the text never
// moves for them; a click on them opens the log of all of them with their
// timings. A real keyboard types into the field as into
// any other; a virtual one has its keys read from what it sends.
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { domStroke } from '../lib/keyfeed'
import { clearTryout, loseHeld, setExpanded, takeStroke, type Tryout } from '../lib/tryout'
import KeyLogPanel from './KeyLogPanel.vue'

const props = defineProps<{
  tryout: Tryout
  /** A virtual keyboard: its keys do not reach the computer this app runs on. */
  virtual: boolean
}>()

const own = ref('')
const text = computed(() => (props.virtual ? props.tryout.text : own.value))
const root = ref<HTMLElement>()
const field = ref<HTMLInputElement>()
const focused = ref(false)
const logOpen = ref(false)
// As many as the line under the field can show; older ones slide out on its left.
const recent = computed(() => props.tryout.log.entries.slice(-14))
const used = computed(() => !!text.value || props.tryout.log.entries.length > 0)

function onKey(event: KeyboardEvent, down: boolean) {
  if (props.virtual) return
  // Tab and Enter would walk off or submit; here they are keys like any other.
  if (down && (event.key === 'Tab' || event.key === 'Enter')) event.preventDefault()
  if (event.repeat) return
  takeStroke(props.tryout, domStroke(event, down), event.timeStamp)
}

function onBlur() {
  focused.value = false
  // Keys let go of elsewhere are not heard here.
  if (!props.virtual) loseHeld(props.tryout)
}

// What a virtual keyboard types is put into the field from outside: keep its end in view.
watch(
  () => props.tryout.text,
  () => nextTick(() => field.value && (field.value.scrollLeft = field.value.scrollWidth)),
)

function clear() {
  own.value = ''
  clearTryout(props.tryout)
  if (!props.tryout.expanded) field.value?.focus()
}

// The log closes as such things do: by its button, by Esc, by a click beside it. Not for a virtual
// keyboard, whose keys are clicked: there it stays open while they are.
function outside(event: PointerEvent) {
  if (!props.virtual && !root.value?.contains(event.target as Node)) logOpen.value = false
}
function escape(event: KeyboardEvent) {
  if (event.key === 'Escape') logOpen.value = false
}
function listen(on: boolean) {
  const change = on ? window.addEventListener : window.removeEventListener
  change('pointerdown', outside as EventListener)
  change('keydown', escape as EventListener)
}
watch(logOpen, listen)
onBeforeUnmount(() => listen(false))
</script>

<template>
  <div ref="root" class="try" :class="{ open: focused || used || logOpen || tryout.expanded }" data-test="scratch" @click="field?.focus()">
    <div class="line">
      <span v-if="tryout.expanded" class="field label muted">Typing test</span>
      <input
        v-else
        ref="field"
        :value="text"
        class="field mono"
        :placeholder="virtual ? 'Press its keys to try it' : 'Type here to try it'"
        :readonly="virtual"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
        data-test="scratch-text"
        @input="own = ($event.target as HTMLInputElement).value"
        @keydown="onKey($event, true)"
        @keyup="onKey($event, false)"
        @focus="focused = true"
        @blur="onBlur"
      />
      <button class="icon clear" :class="{ hidden: !used }" title="Clear the text and the log" aria-label="Clear" :tabindex="used ? 0 : -1" data-test="scratch-clear" @click.stop="clear">×</button>
      <button
        class="icon expand"
        :class="{ on: tryout.expanded }"
        :title="tryout.expanded ? 'Close the typing test' : 'Open the typing test'"
        :aria-label="tryout.expanded ? 'Close the typing test' : 'Open the typing test'"
        :aria-pressed="tryout.expanded"
        data-test="scratch-expand"
        @click.stop="setExpanded(tryout, !tryout.expanded)"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path v-if="tryout.expanded" d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" />
          <path v-else d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />
        </svg>
      </button>
    </div>
    <!-- A line of its own, always there: keys come and go on it without moving the text above. -->
    <button
      class="keys"
      :class="{ on: logOpen }"
      :disabled="!tryout.log.entries.length && !logOpen"
      title="The keys that came last, newest on the right. Click for all of them with their timings."
      aria-label="Timing log"
      :aria-expanded="logOpen"
      data-test="scratch-keys"
      @click.stop="logOpen = !logOpen"
    >
      <span v-if="!recent.length" class="none">keys show here</span>
      <TransitionGroup name="pop">
        <span v-for="key in recent" :key="key.id" class="key">{{ key.label }}</span>
      </TransitionGroup>
    </button>
    <Transition name="fade">
      <KeyLogPanel v-if="logOpen" class="panel" :log="tryout.log" @clear="clearTryout(tryout)" @close="logOpen = false" @click.stop />
    </Transition>
  </div>
</template>

<style scoped>
.try {
  position: relative;
  display: flex;
  flex-direction: column;
  width: 230px;
  /* Never so wide that the header has to make room. */
  flex: 0 1 auto;
  min-width: 150px;
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
  width: min(480px, 40vw);
}

.try:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-soft);
}

.line {
  display: flex;
  align-items: center;
  gap: 2px;
  height: 28px;
  padding: 2px 3px 0 12px;
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

.label {
  display: block;
  color: var(--muted);
  font-size: 0.87rem;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: default;
}

/* The newest key sits at the right end; older ones are pushed out on the left, where they fade. */
.keys {
  display: flex;
  gap: 3px;
  height: 22px;
  align-items: center;
  justify-content: flex-end;
  overflow: hidden;
  padding: 0 6px 2px 12px;
  border: 0;
  border-radius: 0 0 9px 9px;
  background: none;
  cursor: pointer;
  font: inherit;
  mask-image: linear-gradient(to right, transparent 0, #000 36px);
  transition: background 0.15s;
}

.keys:disabled {
  cursor: text;
}

.keys:hover:not(:disabled),
.keys.on {
  background: var(--panel-2);
}

.keys:focus-visible,
.icon:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.none {
  flex: 1;
  text-align: left;
  color: var(--muted);
  font-size: 0.72rem;
  opacity: 0.7;
  white-space: nowrap;
}

.key {
  flex: none;
  padding: 0 6px;
  border-radius: 5px;
  background: var(--cap-top);
  border-bottom: 2px solid var(--cap);
  font-size: 0.72rem;
  font-weight: 600;
  line-height: 15px;
  white-space: nowrap;
}

.key:last-child {
  background: var(--accent);
  color: var(--accent-ink);
}

.icon {
  flex: none;
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border: none;
  border-radius: 7px;
  background: none;
  color: var(--muted);
  font-size: 1.05rem;
  line-height: 1;
  cursor: pointer;
  transition:
    color 0.15s,
    background 0.15s;
}

.icon:hover,
.icon.on {
  color: var(--text);
  background: var(--panel-2);
}

.icon.on {
  color: var(--accent);
}

.icon svg {
  width: 14px;
  height: 14px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}

/* Kept in place while there is nothing to clear, so nothing beside it shifts when there is. */
.clear.hidden {
  visibility: hidden;
}

.panel {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  z-index: 30;
}

@media (max-width: 900px) {
  .try {
    width: 170px;
    min-width: 120px;
  }

  .try.open {
    width: min(480px, 36vw);
  }
}
</style>
