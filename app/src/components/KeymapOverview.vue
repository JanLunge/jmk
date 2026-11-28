<script setup lang="ts">
// Every layer at once, as a picture that can be saved for printing or sharing.
import { computed, ref, watch } from 'vue'
import { attempt, toast } from '../lib/app'
import { keymapPoster, posterPng, saveBlob } from '../lib/poster'
import { knobPositions, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

const MODE_KEY = 'jmk.overview.mode'
const PAPER_KEY = 'jmk.overview.paper'
const mode = ref<'grid' | 'stacked'>(localStorage.getItem(MODE_KEY) === 'stacked' ? 'stacked' : 'grid')
const dark = ref(localStorage.getItem(PAPER_KEY) === 'dark')
watch(mode, (now) => localStorage.setItem(MODE_KEY, now))
watch(dark, (now) => localStorage.setItem(PAPER_KEY, now ? 'dark' : 'light'))

const poster = computed(() =>
  keymapPoster(
    {
      name: props.session.meta.name,
      layout: props.session.layout,
      layers: props.session.layers,
      layerColors: props.session.layerColors,
      layersWith: props.session.layersWith,
      knobs: knobPositions(props.session),
      combos: props.session.combos,
    },
    { mode: mode.value, dark: dark.value },
  ),
)
const fileName = computed(() => (props.session.meta.name || 'keyboard').replace(/[^a-z0-9]+/gi, '-').toLowerCase() + '-keymap')

async function savePng() {
  const blob = await attempt(() => posterPng(poster.value))
  if (!blob) return
  saveBlob(blob, fileName.value + '.png')
  toast('Picture saved')
}

function saveSvg() {
  saveBlob(new Blob([poster.value.svg], { type: 'image/svg+xml' }), fileName.value + '.svg')
  toast('Picture saved')
}
</script>

<template>
  <div class="overview col" data-test="overview">
    <div class="tools row">
      <div class="switch row">
        <button :class="{ on: mode === 'grid' }" data-test="overview-grid" @click="mode = 'grid'">Side by side</button>
        <button :class="{ on: mode === 'stacked' }" data-test="overview-stacked" @click="mode = 'stacked'">On one keyboard</button>
      </div>
      <div class="switch row">
        <button :class="{ on: !dark }" @click="dark = false">Light</button>
        <button :class="{ on: dark }" data-test="overview-dark" @click="dark = true">Dark</button>
      </div>
      <span class="grow" />
      <button class="btn small primary" data-test="save-png" @click="savePng">Save as image</button>
      <button class="btn small" data-test="save-svg" title="A vector file: stays sharp at any size" @click="saveSvg">Save as SVG</button>
    </div>
    <!-- The picture is self-contained SVG made by poster.ts from the keymap; nothing in it comes from outside the app unescaped. -->
    <div class="paper scroll" :class="{ dark }" data-test="poster" v-html="poster.svg" />
    <p class="muted small">
      <template v-if="mode === 'stacked'">The base layer is in the middle of each key; the other layers sit around it in their colours.</template>
      <template v-else>Faint keys are see-through: they do what the base layer says, or a layer theirs is on together with.</template>
      The saved picture is what you see here.
    </p>
  </div>
</template>

<style scoped>
.overview {
  min-height: 0;
  height: 100%;
  gap: 12px;
}

.tools {
  flex-wrap: wrap;
  gap: 10px;
}

.switch {
  gap: 0;
  border: 1px solid var(--line);
  border-radius: 9px;
  overflow: hidden;
}

.switch button {
  padding: 6px 12px;
  border: none;
  background: var(--panel-2);
  font-size: 0.86rem;
  cursor: pointer;
}

.switch button + button {
  border-left: 1px solid var(--line);
}

.switch button.on {
  background: var(--accent);
  color: var(--accent-ink);
  font-weight: 600;
}

.paper {
  flex: 1;
  min-height: 0;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: #ffffff;
  padding: 6px;
}

.paper.dark {
  background: #171717;
}

.paper :deep(svg) {
  display: block;
  max-width: 100%;
  height: auto;
  margin: 0 auto;
}

.small {
  font-size: 0.82rem;
}
</style>
