<script setup lang="ts">
// Where the three rows of letters are, and which arrangement goes on them.
// The rows are shown by dragging along them on the picture of the keyboard.
import { computed, ref } from 'vue'
import KeyboardView from './KeyboardView.vue'
import type { Legend } from '../lib/keycodes'
import { hasKey } from '../lib/layout'
import type { KeyGeometry } from '../lib/protocol'
import { ALPHA_ROW_LENGTH, ALPHA_ROW_NAMES, ALPHAS, rowFrom } from '../lib/starter'

const props = defineProps<{
  layout: KeyGeometry[]
  /** Positions that can hold a letter; every key with a shape when left out. */
  keys?: number[]
  /** Knobs to draw, as in the keymap. */
  knobs?: number[]
}>()
/** The key positions of the top, home and bottom row, left to right. */
const rows = defineModel<number[][]>('rows', { required: true })
/** The id of the arrangement. */
const alphasId = defineModel<string>('alphas', { required: true })

const alphas = computed(() => ALPHAS.find((a) => a.id === alphasId.value) ?? ALPHAS[0])
const paintable = (pos: number) => (props.keys ? props.keys.includes(pos) : hasKey(props.layout[pos])) && !props.knobs?.includes(pos)
const placed = computed(() => rows.value.some((row) => row.length > 0))
/** The row the next drag fills: the first empty one, until the user picks another. */
const paintAt = ref(Math.max(0, rows.value.findIndex((row) => !row.length)))
const stroke = ref<number[]>([])

function legend(pos: number): Legend | undefined {
  const row = rows.value.findIndex((keys) => keys.includes(pos))
  if (row < 0) return { main: '', kind: 'empty' }
  return { main: (alphas.value.rows[row][rows.value[row].indexOf(pos)] ?? '').toUpperCase(), kind: 'key' }
}

const posAt = (event: PointerEvent) => {
  const pos = (document.elementFromPoint(event.clientX, event.clientY)?.closest('[data-pos]') as HTMLElement | null)?.dataset.pos
  return pos === undefined ? undefined : Number(pos)
}
/** Drag along a row to choose its keys; a click on one key takes it and those to its right. */
function down(pos: number) {
  if (!paintable(pos)) return
  stroke.value = [pos]
  const move = (event: PointerEvent) => {
    const over = posAt(event)
    if (over !== undefined && paintable(over) && !stroke.value.includes(over)) stroke.value = [...stroke.value, over]
  }
  const up = () => {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', up)
    const touched = stroke.value
    stroke.value = []
    const row =
      touched.length > 1
        ? [...touched].sort((a, b) => props.layout[a].x - props.layout[b].x).slice(0, ALPHA_ROW_LENGTH)
        : rowFrom(props.layout, touched[0]).filter(paintable)
    // A key holds one letter: it leaves the row it was in before.
    const next = rows.value.map((keys, index) => (index === paintAt.value ? row : keys.filter((key) => !row.includes(key))))
    rows.value = next
    const open = next.findIndex((keys) => !keys.length)
    if (open >= 0) paintAt.value = open
  }
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up)
}
function clear() {
  rows.value = [[], [], []]
  paintAt.value = 0
}
const spaced = (text: string) => text.toUpperCase().split('').join(' ')
</script>

<template>
  <div class="letters col">
    <div class="row top">
      <div class="seg row">
        <button v-for="a in ALPHAS" :key="a.id" class="seg-btn" :class="{ on: alphasId === a.id }" :data-test="'alphas-' + a.id" @click="alphasId = a.id">{{ a.name }}</button>
      </div>
      <button v-if="placed" class="btn small ghost" data-test="letters-clear" @click="clear">Start over</button>
    </div>
    <div class="row rows">
      <button
        v-for="(name, row) in ALPHA_ROW_NAMES"
        :key="row"
        class="letter-row col"
        :class="{ on: paintAt === row, done: rows[row].length > 0 }"
        :title="'Click, then drag along this row on the keyboard'"
        :data-test="'letter-row-' + row"
        @click="paintAt = row"
      >
        <span class="small">{{ name }}<template v-if="rows[row].length"> · {{ rows[row].length }} keys</template></span>
        <span class="mono">{{ spaced(alphas.rows[row]) }}</span>
      </button>
    </div>
    <p class="muted small hint" data-test="letters-hint">
      Drag along the row that holds <strong class="mono">{{ spaced(alphas.rows[paintAt].slice(0, 5)) }}</strong>…, or click its first key.
    </p>
    <div class="draft">
      <KeyboardView :layout="layout" :legend="legend" :selected="stroke" :knobs="knobs ?? []" interactive data-test="letters-board" @down="down" />
    </div>
  </div>
</template>

<style scoped>
.letters {
  gap: 10px;
  align-self: stretch;
  align-items: center;
  min-width: 0;
}

.top {
  gap: 12px;
  justify-content: center;
  align-items: center;
  flex-wrap: wrap;
}

.seg {
  padding: 2px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: var(--panel-2);
}

.seg-btn {
  padding: 6px 14px;
  border: none;
  border-radius: 8px;
  background: none;
  color: var(--muted);
  cursor: pointer;
  white-space: nowrap;
}

.seg-btn.on {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}

.rows {
  gap: 8px;
  justify-content: center;
  flex-wrap: wrap;
}

.letter-row {
  gap: 2px;
  padding: 7px 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel);
  color: var(--muted);
  text-align: left;
  cursor: pointer;
}

.letter-row .mono {
  font-size: 0.8rem;
  letter-spacing: 0.04em;
}

.letter-row.done {
  color: var(--text);
}

.letter-row.on {
  border-color: var(--accent);
  color: var(--accent);
}

.small {
  font-size: 0.82rem;
}

.hint {
  margin: 0;
}

.draft {
  align-self: stretch;
  width: 100%;
  max-width: 820px;
  margin: 0 auto;
  display: grid;
  place-items: center;
}
</style>
