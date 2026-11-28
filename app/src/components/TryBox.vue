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
import { speedStats } from '../lib/speed'
import KeyLogPanel from './KeyLogPanel.vue'
import SpeedPanel from './SpeedPanel.vue'

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
const speedOpen = ref(false)
/** The speed of the burst typed last, once there is one long enough to have a speed. */
const speedNow = computed(() => speedStats(props.tryout.speed).last)
const strip = ref<HTMLElement>()
/** More keys than the line holds: the oldest then run out on its left. */
const full = ref(false)
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
  if (root.value?.contains(event.target as Node)) return
  if (!props.virtual) logOpen.value = false
  speedOpen.value = false
}
function escape(event: KeyboardEvent) {
  if (event.key === 'Escape') logOpen.value = speedOpen.value = false
}
function listen(on: boolean) {
  const change = on ? window.addEventListener : window.removeEventListener
  change('pointerdown', outside as EventListener)
  change('keydown', escape as EventListener)
}
watch(() => logOpen.value || speedOpen.value, listen)
// One thing hangs from the field at a time.
watch(logOpen, (open) => open && (speedOpen.value = false))
watch(speedOpen, (open) => open && (logOpen.value = false))
watch(
  () => props.tryout.log.entries.length,
  () => nextTick(() => strip.value && (full.value = strip.value.scrollWidth > strip.value.clientWidth + 1)),
)
onBeforeUnmount(() => listen(false))
</script>

<template>
  <div ref="root" class="try" :class="{ open: focused || used || logOpen || speedOpen || tryout.expanded }" data-test="scratch" @click="field?.focus()">
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
    <div class="under">
    <button
      ref="strip"
      class="keys"
      :class="{ on: logOpen, full }"
      :disabled="!tryout.log.entries.length && !logOpen"
      title="The keys in the order they came. Click for all of them with their timings."
      aria-label="Timing log"
      :aria-expanded="logOpen"
      data-test="scratch-keys"
      @click.stop="logOpen = !logOpen"
    >
      <span v-if="!recent.length" class="none">keys show here</span>
      <TransitionGroup name="pop" tag="span" class="track">
        <span v-for="key in recent" :key="key.id" class="key">{{ key.label }}</span>
      </TransitionGroup>
    </button>
    <button
      class="wpm mono"
      :class="{ on: speedOpen }"
      :disabled="speedNow === undefined && !speedOpen"
      title="How fast the last stretch of typing was. Click for more."
      aria-label="Typing speed"
      :aria-expanded="speedOpen"
      data-test="scratch-speed"
      @click.stop="speedOpen = !speedOpen"
    >
      <strong>{{ speedNow === undefined ? '–' : Math.round(speedNow) }}</strong> wpm
    </button>
    </div>
    <Transition name="fade">
      <KeyLogPanel v-if="logOpen" class="panel" :log="tryout.log" @clear="clearTryout(tryout)" @close="logOpen = false" @click.stop />
      <SpeedPanel v-else-if="speedOpen" class="panel" :speed="tryout.speed" @close="speedOpen = false" @click.stop />
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

.under {
  display: flex;
  align-items: stretch;
  height: 22px;
}

/* Keys line up from the left as they come. Once the line is full it holds on to its right end, and the oldest run out on the left. */
.keys {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  overflow: hidden;
  padding: 0 4px 2px 12px;
  border: 0;
  border-radius: 0 0 0 9px;
  background: none;
  cursor: pointer;
  font: inherit;
  transition: background 0.15s;
}

.keys.full {
  mask-image: linear-gradient(to right, transparent 4px, #000 40px);
}

.track {
  flex: none;
  min-width: 100%;
  display: flex;
  align-items: center;
  gap: 3px;
}

/* Nothing pressed yet: the hint has the line. */
.track:empty {
  display: none;
}

.wpm {
  flex: none;
  min-width: 62px;
  padding: 0 9px 2px 6px;
  border: 0;
  border-radius: 0 0 9px 0;
  background: none;
  color: var(--muted);
  font-size: 0.72rem;
  text-align: right;
  white-space: nowrap;
  cursor: pointer;
  transition:
    color 0.15s,
    background 0.15s;
}

.wpm strong {
  color: var(--text);
  font-variant-numeric: tabular-nums;
}

.wpm:disabled {
  cursor: text;
  opacity: 0.6;
}

.wpm:hover:not(:disabled),
.wpm.on {
  background: var(--panel-2);
}

.keys:disabled {
  cursor: text;
}

.keys:hover:not(:disabled),
.keys.on {
  background: var(--panel-2);
}

.keys:focus-visible,
.wpm:focus-visible,
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
