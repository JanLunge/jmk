<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import ActionPicker from '../components/ActionPicker.vue'
import KeyboardView from '../components/KeyboardView.vue'
import { attempt } from '../lib/app'
import { localPositions } from '../lib/definition'
import { legend } from '../lib/keycodes'
import { hasKey } from '../lib/layout'
import type { Action } from '../lib/protocol'
import { positionCount, setAction, setLayerCount, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

const layer = ref(0)
const selected = ref<number | undefined>(props.session.layout.findIndex(hasKey))
watch(() => props.session.layers.length, (count) => (layer.value = Math.min(layer.value, count - 1)))

const actions = computed(() => props.session.layers[layer.value] ?? [])
const keyLegend = (pos: number) => {
  const action = actions.value[pos]
  if (action === undefined) return undefined
  // A see-through key shows, faintly, what it falls through to.
  if (action === 'Transparent') {
    for (let below = layer.value - 1; below >= 0; below--) {
      const lower = props.session.layers[below][pos]
      if (lower !== 'Transparent') return { ...legend(lower), kind: 'through' as const }
    }
  }
  return legend(action)
}

/** Knob directions live after this half's switches in the keymap. */
const knobs = computed(() => {
  const count = props.session.hardware.encoders.length
  const first = localPositions(props.session.hardware) - 2 * count
  return [...Array(count).keys()].map((index) => ({ index, ccw: first + 2 * index, cw: first + 2 * index + 1 }))
})

async function change(action: Action) {
  if (selected.value === undefined) return
  await attempt(() => setAction(props.session, layer.value, selected.value!, action))
}

const maxLayers = props.session.info.max_layers
const addLayer = () => attempt(() => setLayerCount(props.session, props.session.layers.length + 1)).then(() => (layer.value = props.session.layers.length - 1))
const removeLayer = () => attempt(() => setLayerCount(props.session, props.session.layers.length - 1))
const selectedAction = computed(() => (selected.value === undefined ? undefined : actions.value[selected.value]))
const missingRemote = computed(() => props.session.layout.length > positionCount(props.session))
</script>

<template>
  <div class="keymap">
    <div class="stage col">
      <div class="layers row" data-test="layers">
        <TransitionGroup name="pop">
          <button v-for="(_, index) in session.layers" :key="index" class="layer" :class="{ on: layer === index }" :data-test="'layer-' + index" @click="layer = index">
            <span class="sheet" />
            Layer {{ index }}
            <span v-if="index === 0" class="muted small">base</span>
          </button>
        </TransitionGroup>
        <button class="btn small" :disabled="session.layers.length >= maxLayers" data-test="add-layer" @click="addLayer">+ layer</button>
        <button v-if="session.layers.length > 1" class="btn small ghost" data-test="remove-layer" @click="removeLayer">remove last</button>
      </div>

      <div class="board scroll">
        <KeyboardView
          :layout="session.layout"
          :legend="keyLegend"
          :pressed="session.pressed"
          :selected="selected === undefined ? [] : [selected]"
          interactive
          data-test="keymap-board"
          @down="(pos) => (selected = pos)"
        />
        <div v-if="knobs.length" class="knobs row">
          <div v-for="knob in knobs" :key="knob.index" class="knob row">
            <button class="turn" :class="{ on: selected === knob.ccw }" :data-test="'knob-' + knob.index + '-ccw'" @click="selected = knob.ccw">
              ⟲ <strong>{{ keyLegend(knob.ccw)?.main || '·' }}</strong>
            </button>
            <span class="dial" />
            <button class="turn" :class="{ on: selected === knob.cw }" :data-test="'knob-' + knob.index + '-cw'" @click="selected = knob.cw">
              <strong>{{ keyLegend(knob.cw)?.main || '·' }}</strong> ⟳
            </button>
          </div>
        </div>
      </div>
      <p class="muted small hint">
        Click a key, then pick what it does on the right. Changes work at once; press keys on your keyboard to try
        them. They stay after unplugging once you save.
        <template v-if="missingRemote"> The other half is not connected right now; its keys can still be edited.</template>
      </p>
    </div>

    <aside class="side card">
      <ActionPicker
        v-if="selectedAction !== undefined"
        :action="selectedAction"
        :layer-count="session.layers.length"
        :current-layer="layer"
        :macros="session.macros.map((m) => m.length > 0)"
        @change="change"
      />
      <p v-else class="muted">Select a key.</p>
    </aside>
  </div>
</template>

<style scoped>
.keymap {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 380px;
  gap: 18px;
  height: 100%;
  min-height: 0;
  padding: 18px 24px;
}

.stage {
  min-height: 0;
  gap: 14px;
}

.layers {
  flex-wrap: wrap;
}

.layer {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 14px;
  border-radius: 10px;
  border: 1px solid var(--line);
  background: var(--panel);
  cursor: pointer;
  transition:
    border-color 0.15s,
    background 0.15s,
    transform 0.15s var(--ease);
}

.layer:hover {
  transform: translateY(-1px);
}

.layer.on {
  border-color: var(--accent);
  background: var(--accent-soft);
}

.sheet {
  width: 14px;
  height: 10px;
  border-radius: 3px;
  border: 2px solid currentColor;
  transform: skewX(-14deg);
  opacity: 0.6;
}

.small {
  font-size: 0.82rem;
}

.board {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 18px;
  padding: 10px;
}

.board > svg {
  flex: none;
}

.knob {
  gap: 6px;
}

.turn {
  padding: 7px 12px;
  border-radius: 10px;
  border: 2px solid transparent;
  background: var(--cap-top);
  cursor: pointer;
}

.turn.on {
  border-color: var(--accent);
}

.dial {
  width: 34px;
  height: 34px;
  border-radius: 50%;
  background: conic-gradient(from 20deg, var(--cap), var(--cap-top), var(--cap));
  border: 2px solid var(--line);
}

.hint {
  text-align: center;
}

.side {
  min-height: 0;
  padding: 16px;
}

@media (max-width: 980px) {
  .keymap {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(0, 1fr) minmax(0, 1fr);
  }
}
</style>
