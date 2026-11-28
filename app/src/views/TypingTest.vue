<script setup lang="ts">
// The typing test: words to type against the clock (or just typing, with the
// speed shown) above the keyboard's own layout, which follows the layer that
// is active, so a layout can be learned by using it.
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import KeyboardView from '../components/KeyboardView.vue'
import SpeedChart from '../components/SpeedChart.vue'
import { domStroke, type Stroke } from '../lib/keyfeed'
import { keyFor } from '../lib/language'
import { heldOn, layerColor, layerLegend, topLayer, whereIs } from '../lib/layers'
import { knobPositions, type Session } from '../lib/session'
import { takeStroke, type Tryout } from '../lib/tryout'
import { backspace, isDone, isFree, isRunning, keyStats, liveWpm, missedMost, newTest, slowest, stats, tick, typeChar } from '../lib/typingtest'

const props = defineProps<{
  session: Session
  tryout: Tryout
  /** A virtual keyboard's keys are read from what it sends; a real one types into this page. */
  virtual: boolean
}>()
defineEmits<{ close: [] }>()

const MODES = [
  { duration: 0, label: 'Freeform', title: 'No words to type and no clock: just type, and see the speed' },
  { duration: 30, label: '30 seconds', title: 'Type the words for 30 seconds' },
  { duration: 60, label: '60 seconds', title: 'Type the words for 60 seconds' },
]
const MODE_KEY = 'jmk.typing.mode'
const keptMode = Number(localStorage.getItem(MODE_KEY) ?? 30)
const duration = ref(MODES.some((mode) => mode.duration === keptMode) ? keptMode : 30)
const test = ref(newTest(duration.value))
const now = ref(performance.now())

function restart() {
  test.value = newTest(duration.value)
  now.value = performance.now()
  focus()
}
watch(duration, (seconds) => {
  localStorage.setItem(MODE_KEY, String(seconds))
  restart()
})

const free = computed(() => isFree(test.value))
const running = computed(() => isRunning(test.value))
const done = computed(() => isDone(test.value))
const figures = computed(() => stats(test.value, now.value))
const left = computed(() => Math.max(0, Math.ceil(test.value.duration - figures.value.seconds)))
const pace = computed(() => liveWpm(test.value, now.value))
const keys = computed(() => (done.value ? keyStats(test.value) : []))
const missed = computed(() => missedMost(keys.value))
const slow = computed(() => slowest(keys.value))
const shownChar = (char: string) => (char === ' ' ? '␣' : char)
const clock = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`

// --- Keys ------------------------------------------------------------------------
/** Every key, from either kind of keyboard. What a real one types comes through the field below instead. */
function hear(stroke: Stroke, at: number) {
  if (!stroke.down) return
  if (stroke.kind === 'tab' || stroke.kind === 'escape') return restart()
  if (stroke.kind === 'backspace') return backspace(test.value, at, stroke.word)
  if (stroke.kind === 'enter') return free.value ? typeChar(test.value, '\n', at) : undefined
  if (stroke.text) typeChar(test.value, stroke.text, at)
}

const catcher = ref<HTMLInputElement>()
const focused = ref(false)
const focus = () => !props.virtual && catcher.value?.focus({ preventScroll: true })
/** When the key was pressed, on the clock of performance.now(). */
const stamp = (event: Event) => event.timeStamp || performance.now()

function onKey(event: KeyboardEvent, down: boolean) {
  if (down && event.key === 'Tab') event.preventDefault()
  // A key held down repeats: Backspace then takes more, and is still one press.
  if (event.repeat) return void (down && event.key === 'Backspace' && !event.isComposing && backspace(test.value, stamp(event)))
  const stroke = domStroke(event, down)
  // While an accent is being put on a letter, the keys belong to that.
  takeStroke(props.tryout, event.isComposing ? { ...stroke, kind: 'other' } : stroke, stamp(event))
}

/** What the computer made of the keys: the field is emptied as it fills. */
function onInput(event: Event) {
  if ((event as InputEvent).isComposing) return
  takeText(event)
}
function takeText(event: Event) {
  const field = event.target as HTMLInputElement
  const text = field.value
  field.value = ''
  for (const char of text) typeChar(test.value, char, stamp(event))
}

/** A key pressed with nothing in focus is meant for the test. */
function onStray(event: KeyboardEvent) {
  if (props.virtual || document.activeElement === catcher.value) return
  if ((event.target as HTMLElement).closest?.('input, textarea, select, [contenteditable]') || event.metaKey || event.ctrlKey) return
  focus()
}

let timer: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  props.tryout.hear = hear
  window.addEventListener('keydown', onStray, true)
  timer = setInterval(() => {
    now.value = performance.now()
    tick(test.value, now.value)
  }, 100)
  focus()
})
onBeforeUnmount(() => {
  props.tryout.hear = undefined
  window.removeEventListener('keydown', onStray, true)
  clearInterval(timer)
  observer?.disconnect()
})

// --- The words ---------------------------------------------------------------------
const current = computed(() => test.value.typed.length - 1)
const words = computed(() => {
  const { words, typed } = test.value
  return words.slice(0, typed.length + 45).map((word, index) => {
    const got = typed[index] ?? ''
    const past = index < current.value
    const letters: { char: string; state: 'ok' | 'bad' | 'extra' | 'missed' | 'todo' }[] = []
    for (let at = 0; at < Math.max(word.length, got.length); at++) {
      if (at >= word.length) letters.push({ char: got[at], state: 'extra' })
      else if (at >= got.length) letters.push({ char: word[at], state: past ? 'missed' : 'todo' })
      else letters.push({ char: word[at], state: got[at] === word[at] ? 'ok' : 'bad' })
    }
    return { index, letters, wrong: past && got !== word }
  })
})

// The caret is one mark that slides from letter to letter, and the lines move up under it.
const lines = ref<HTMLElement>()
const caret = reactive({ x: 0, y: 0, shown: false })
const scrolled = ref(0)
function place() {
  const word = lines.value?.querySelector<HTMLElement>('.word.current')
  if (!word) return void (caret.shown = false)
  const letters = word.querySelectorAll<HTMLElement>('.letter')
  const at = test.value.typed[current.value].length
  const last = letters[letters.length - 1]
  caret.x = letters[at] ? letters[at].offsetLeft : last ? last.offsetLeft + last.offsetWidth : word.offsetLeft
  caret.y = word.offsetTop
  caret.shown = true
  // The line being typed is the second of the three shown.
  scrolled.value = Math.max(0, word.offsetTop - word.offsetHeight)
}
watch(() => [test.value, current.value, test.value.typed[current.value], done.value], () => nextTick(place), { flush: 'post' })
let observer: ResizeObserver | undefined
watch(lines, (box) => {
  observer?.disconnect()
  if (!box || typeof ResizeObserver === 'undefined') return
  observer = new ResizeObserver(place)
  observer.observe(box)
  place()
})

/** A freeform run fills its three lines and then moves up, the newest line staying in view. */
const page = ref<HTMLElement>()
watch(() => test.value.typed[0], () => nextTick(() => page.value && (page.value.scrollTop = page.value.scrollHeight)), { flush: 'post' })

// --- The layout below ----------------------------------------------------------------
/** The layer the user chose to look at; none while the picture follows the keyboard. */
const pinned = ref<number>()
const following = computed(() => pinned.value === undefined && props.session.layerAware)
const layer = computed(() => Math.min(pinned.value ?? topLayer(props.session) ?? 0, props.session.layers.length - 1))
const layerActive = (index: number) => props.session.layerAware && (props.session.activeLayers & (1 << index)) !== 0
const legendAt = (pos: number) => layerLegend(props.session, layer.value, pos)
const tintAt = (pos: number) => {
  const target = legendAt(pos)?.layer
  return target === undefined ? undefined : layerColor(props.session, target)
}
const held = computed(() => heldOn(props.session, layer.value))

const HINT_KEY = 'jmk.typing.hint'
const hinting = ref(localStorage.getItem(HINT_KEY) !== 'off')
watch(hinting, (on) => localStorage.setItem(HINT_KEY, on ? 'on' : 'off'))
const find = (char: string) => {
  const key = char === ' ' ? { usage: 0x2c, mods: 0 } : keyFor(char)
  return key && whereIs(props.session, layer.value, key.usage, key.mods)
}
/** The character due next, and where its key is. */
const due = computed(() => {
  if (free.value || done.value) return undefined
  const word = test.value.words[current.value]
  const char = word[test.value.typed[current.value].length] ?? ' '
  return { char, at: find(char) }
})
/** Outlined in the picture: the key to press next, or after a test the keys missed most. */
const marked = computed(() => {
  if (done.value) return missed.value.flatMap((key) => (find(key.char)?.layer === layer.value ? [find(key.char)!.pos] : []))
  return hinting.value && due.value?.at?.layer === layer.value ? [due.value.at.pos] : []
})
const hint = computed(() => {
  if (done.value) return marked.value.length ? 'Outlined: the keys missed most.' : ''
  if (!hinting.value || !due.value) return ''
  const name = due.value.char === ' ' ? 'Space' : `“${due.value.char}”`
  if (!due.value.at) return `No key of this keyboard types ${name}.`
  return due.value.at.layer === layer.value ? '' : `${name} is on layer ${due.value.at.layer}.`
})
</script>

<template>
  <div class="typing" :class="{ done }" data-test="typing-test" @click="focus">
    <div class="bar row">
      <div class="modes row" role="radiogroup" aria-label="Kind of test">
        <button
          v-for="mode in MODES"
          :key="mode.duration"
          class="mode"
          :class="{ on: duration === mode.duration }"
          role="radio"
          :aria-checked="duration === mode.duration"
          :title="mode.title"
          :data-test="'test-mode-' + mode.duration"
          @click="duration = mode.duration"
        >
          {{ mode.label }}
        </button>
      </div>
      <span class="grow" />
      <button class="btn small" :class="done ? 'primary' : 'ghost'" title="Start again (Tab or Esc)" data-test="test-restart" @click="restart">{{ done ? 'Again' : 'Restart' }} <kbd>Tab</kbd></button>
      <button class="btn small" data-test="test-close" @click.stop="$emit('close')">Back to the editor</button>
    </div>

    <section class="stage" :class="{ idle: !running, blurred: !virtual && !focused && !done }">
      <input
        v-if="!virtual"
        ref="catcher"
        class="catcher"
        aria-label="Type here"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
        autocorrect="off"
        data-test="test-input"
        @keydown="onKey($event, true)"
        @keyup="onKey($event, false)"
        @input="onInput"
        @compositionend="takeText"
        @focus="focused = true"
        @blur="focused = false"
      />

      <div v-if="done" class="result" data-test="test-result">
        <div class="headline">
          <div class="big">
            <span class="unit">wpm</span>
            <strong data-test="result-wpm">{{ Math.round(figures.wpm) }}</strong>
          </div>
          <div class="big">
            <span class="unit">accuracy</span>
            <strong data-test="result-accuracy">{{ Math.round(figures.accuracy) }}%</strong>
          </div>
          <dl class="small-facts">
            <div title="The speed with the mistakes counted in"><dt>raw</dt><dd data-test="result-raw">{{ Math.round(figures.raw) }}</dd></div>
            <div title="Right, wrong, typed too many, left out"><dt>characters</dt><dd>{{ figures.correct }} / {{ figures.incorrect }} / {{ figures.extra }} / {{ figures.missed }}</dd></div>
            <div><dt>time</dt><dd>{{ test.duration }} s</dd></div>
          </dl>
        </div>
        <SpeedChart :samples="test.samples" />
        <div class="col lists" data-test="result-keys">
          <div>
            <h3>Missed most</h3>
            <p v-if="!missed.length" class="muted small">No key missed.</p>
            <ul v-else class="keylist">
              <li v-for="key in missed" :key="key.char" data-test="missed-key"><span class="cap">{{ shownChar(key.char) }}</span><span class="mono muted">{{ key.misses }} of {{ key.count }}</span></li>
            </ul>
          </div>
          <div v-if="slow.length">
            <h3>Slowest</h3>
            <ul class="keylist">
              <li v-for="key in slow" :key="key.char" data-test="slow-key"><span class="cap">{{ shownChar(key.char) }}</span><span class="mono muted">{{ Math.round(key.ms!) }} ms</span></li>
            </ul>
          </div>
        </div>
      </div>

      <template v-else>
        <div class="meter row">
          <template v-if="free">
            <span class="count" data-test="test-live">{{ Math.round(pace) }}</span>
            <span class="muted">wpm now</span>
            <span class="fact"><strong data-test="test-average">{{ Math.round(figures.wpm) }}</strong> <span class="muted">average</span></span>
            <span class="fact"><strong data-test="test-chars">{{ figures.correct }}</strong> <span class="muted">characters</span></span>
            <span class="fact"><strong>{{ clock(figures.seconds) }}</strong> <span class="muted">typing</span></span>
          </template>
          <template v-else>
            <span class="count" :class="{ waiting: !running }" data-test="test-clock">{{ left }}</span>
            <span v-if="running" class="fact"><strong data-test="test-live">{{ Math.round(figures.wpm) }}</strong> <span class="muted">wpm</span></span>
            <span v-else class="muted">seconds, from your first key</span>
          </template>
        </div>

        <div v-if="free" ref="page" class="free mono" data-test="test-free">
          <p><span v-if="!test.typed[0]" class="ghost">Type anything.</span>{{ test.typed[0] }}<i class="caret inline" /></p>
        </div>
        <div v-else class="window" data-test="test-words">
          <div ref="lines" class="lines mono" :style="{ transform: `translateY(${-scrolled}px)` }">
            <i v-if="caret.shown" class="caret" :style="{ transform: `translate(${caret.x}px, ${caret.y}px)` }" />
            <span v-for="word in words" :key="word.index" class="word" :class="{ current: word.index === current, wrong: word.wrong }" :data-word="word.index">
              <span v-for="(letter, at) in word.letters" :key="at" class="letter" :class="letter.state">{{ letter.char }}</span>
            </span>
          </div>
        </div>
      </template>

      <button v-if="!virtual && !focused && !done" class="wake" data-test="test-focus" @click.stop="focus">Click here or press a key to type</button>
    </section>

    <section class="layout col">
      <div class="layers row" data-test="test-layers">
        <button
          v-if="session.layerAware"
          class="layer follow"
          :class="{ on: following }"
          :aria-pressed="following"
          title="Show whichever layer is active on the keyboard"
          data-test="test-follow"
          @click="pinned = undefined"
        >
          <span class="live-dot" /> Follow the keyboard
        </button>
        <button
          v-for="(_, index) in session.layers"
          :key="index"
          class="layer"
          :class="{ on: layer === index, pinned: pinned === index, live: index > 0 && layerActive(index) }"
          :style="{ '--tint': layerColor(session, index) ?? 'var(--muted)' }"
          :title="pinned === index ? 'Kept in view. Click again to follow the keyboard.' : 'Keep this layer in view'"
          :aria-pressed="pinned === index"
          :data-test="'test-layer-' + index"
          @click="pinned = pinned === index ? undefined : index"
        >
          <span class="swatch" />
          Layer {{ index }}
          <span v-if="pinned === index" class="muted small">kept</span>
          <span v-if="index > 0 && layerActive(index)" class="live-dot" title="Active on the keyboard right now" />
        </button>
        <span class="grow" />
        <label v-if="!free" class="row hinting small" title="Outline the key that types the next character">
          <input v-model="hinting" type="checkbox" data-test="test-hint" />
          Show the next key
        </label>
      </div>
      <div class="board">
        <KeyboardView :layout="session.layout" :legend="legendAt" :tint="tintAt" :pressed="held.here" :pressed-elsewhere="held.elsewhere" :selected="marked" :knobs="knobPositions(session)" :max-unit="58" data-test="test-board" />
      </div>
      <!-- The room for this is always kept: a line that comes and goes would make the keyboard jump. -->
      <p class="hint muted small" data-test="test-hint-text">{{ hint }}</p>
    </section>
  </div>
</template>

<style scoped>
.typing {
  display: grid;
  /* Half and half; in a window too low for both, the page scrolls before either is squeezed to nothing. */
  grid-template-rows: auto minmax(200px, 1fr) minmax(180px, 1fr);
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  scrollbar-width: thin;
}

/* A result that needs more than its half takes it from the layout. */
.typing.done {
  grid-template-rows: auto minmax(min-content, 1fr) minmax(180px, 1fr);
}

.bar {
  padding: 5px 24px 6px;
  border-bottom: 1px solid var(--line);
  flex-wrap: wrap;
  gap: 8px;
}

.modes {
  gap: 2px;
  padding: 3px;
  border-radius: 10px;
  background: var(--panel-2);
  border: 1px solid var(--line);
}

.mode {
  padding: 3px 12px;
  border: 0;
  border-radius: 7px;
  background: none;
  color: var(--muted);
  font-size: 0.87rem;
  cursor: pointer;
  white-space: nowrap;
  transition:
    background 0.15s,
    color 0.15s;
}

.mode:hover {
  color: var(--text);
}

.mode.on {
  background: var(--panel);
  color: var(--text);
  font-weight: 600;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.18);
}

.btn.primary kbd {
  background: none;
  border-color: currentColor;
  color: inherit;
  opacity: 0.8;
}

kbd {
  padding: 0 5px;
  border-radius: 5px;
  border: 1px solid var(--line);
  border-bottom-width: 2px;
  background: var(--panel);
  color: var(--muted);
  font: inherit;
  font-size: 0.74rem;
}

.small {
  font-size: 0.82rem;
}

/* --- The test ---------------------------------------------------------------- */
.stage {
  --pitch: clamp(2.1rem, 3.6vw, 2.7rem);
  position: relative;
  min-height: 0;
  overflow: auto;
  scrollbar-width: thin;
  display: flex;
  flex-direction: column;
  justify-content: safe center;
  gap: 10px;
  padding: 14px max(24px, calc((100% - 980px) / 2));
}

.catcher {
  position: absolute;
  top: 50%;
  left: 50%;
  width: 1px;
  height: 1px;
  padding: 0;
  border: 0;
  opacity: 0;
  pointer-events: none;
}

.meter {
  gap: 10px;
  align-items: baseline;
  min-height: 2.6rem;
  flex-wrap: wrap;
}

.count {
  font-size: 2.1rem;
  font-weight: 700;
  line-height: 1;
  color: var(--accent);
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.02em;
}

.count.waiting {
  color: var(--muted);
}

.fact {
  margin-left: 14px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.window {
  height: calc(var(--pitch) * 3);
  overflow: hidden;
  flex: none;
}

.lines {
  position: relative;
  display: flex;
  flex-wrap: wrap;
  column-gap: 0.62em;
  font-size: calc(var(--pitch) * 0.6);
  line-height: var(--pitch);
  transition: transform 0.2s var(--ease);
  user-select: none;
}

.word {
  display: inline-flex;
  height: var(--pitch);
  background: linear-gradient(transparent, transparent) no-repeat 0 calc(100% - 0.28em) / 100% 2px;
}

.word.wrong {
  background-image: linear-gradient(var(--danger), var(--danger));
}

.letter {
  color: color-mix(in srgb, var(--muted) 62%, transparent);
  transition: color 0.08s;
}

.letter.ok {
  color: var(--text);
}

.letter.bad {
  color: var(--danger);
}

.letter.extra {
  color: color-mix(in srgb, var(--danger) 62%, var(--bg));
}

.letter.missed {
  color: color-mix(in srgb, var(--muted) 62%, transparent);
}

.caret {
  position: absolute;
  left: -1px;
  top: calc(var(--pitch) * 0.18);
  width: 2.5px;
  height: calc(var(--pitch) * 0.64);
  border-radius: 2px;
  background: var(--accent);
  transition: transform 0.1s var(--ease);
  pointer-events: none;
}

.idle .caret {
  animation: blink 1.1s ease-in-out infinite;
}

.caret.inline {
  position: static;
  display: inline-block;
  height: 1.15em;
  margin-left: 1px;
  vertical-align: text-bottom;
}

@keyframes blink {
  50% {
    opacity: 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .caret,
  .lines {
    transition: none;
  }

  /* A caret that stands still, not one that flickers. */
  .idle .caret {
    animation: none;
  }
}

.free {
  height: calc(var(--pitch) * 3);
  flex: none;
  overflow: hidden;
  font-size: calc(var(--pitch) * 0.6);
  line-height: var(--pitch);
}

.free p {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.ghost {
  color: color-mix(in srgb, var(--muted) 62%, transparent);
}

.blurred .window,
.blurred .free {
  filter: blur(4px);
  opacity: 0.55;
}

.wake {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  border: 0;
  background: none;
  color: var(--text);
  font-weight: 600;
  cursor: pointer;
}

/* --- The result ---------------------------------------------------------------- */
.result {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 14px 32px;
}

.headline {
  display: grid;
  grid-template-columns: auto auto;
  gap: 4px 28px;
  align-items: end;
}

.big {
  display: flex;
  flex-direction: column;
}

.unit {
  font-size: 0.82rem;
  color: var(--muted);
}

.big strong {
  font-size: clamp(2.6rem, 5vw, 3.6rem);
  line-height: 1;
  font-weight: 700;
  letter-spacing: -0.03em;
  color: var(--accent);
  font-variant-numeric: tabular-nums;
}

.small-facts {
  grid-column: 1 / -1;
  display: flex;
  gap: 18px;
  margin: 6px 0 0;
  font-size: 0.82rem;
}

.small-facts dt {
  color: var(--muted);
}

.small-facts dd {
  margin: 0;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.result .chart {
  flex: 1 1 260px;
}

.lists {
  gap: 10px;
  min-width: 190px;
}

.lists h3 {
  font-size: 0.78rem;
  font-weight: 500;
  color: var(--muted);
  margin-bottom: 4px;
}

.keylist {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 5px 12px;
  max-width: 300px;
}

.keylist li {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  white-space: nowrap;
}

.cap {
  min-width: 24px;
  text-align: center;
  padding: 1px 7px;
  border-radius: 6px;
  background: var(--cap-top);
  border-bottom: 2px solid var(--cap);
  font-size: 0.82rem;
  font-weight: 600;
}

/* --- The layout ---------------------------------------------------------------- */
.layout {
  min-height: 0;
  gap: 6px;
  padding: 10px 24px 8px;
  border-top: 1px solid var(--line);
  background: color-mix(in srgb, var(--panel) 55%, var(--bg));
}

.layers {
  flex-wrap: wrap;
  gap: 8px;
}

.layer {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 12px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: var(--panel);
  font-size: 0.9rem;
  cursor: pointer;
  white-space: nowrap;
  transition:
    border-color 0.15s,
    background 0.15s;
}

.layer.on {
  border-color: var(--tint, var(--signal));
  background: color-mix(in srgb, var(--tint, var(--signal)) 16%, var(--panel));
}

.layer.live:not(.on) {
  border-color: var(--signal);
}

.swatch {
  width: 12px;
  height: 12px;
  border-radius: 4px;
  background: var(--tint);
}

.live-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--signal);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--signal) 25%, transparent);
}

.follow:not(.on) .live-dot {
  background: var(--muted);
  box-shadow: none;
}

.hinting {
  gap: 6px;
  color: var(--muted);
  cursor: pointer;
  white-space: nowrap;
}

.hinting input {
  accent-color: var(--accent);
}

.board {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 0 10px;
}

/* The drawing shrinks to the room there is, in both directions. */
.board > svg {
  /* The same picture in the keymap and under the typing test: it moves from one to the other. */
  view-transition-name: keyboard;
  flex: 1 1 0;
  min-height: 0;
}

.hint {
  flex: none;
  height: 1.3rem;
  text-align: center;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
</style>
