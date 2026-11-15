<script setup lang="ts">
// The stand-in for a physical keyboard: click its keys to "press" them.
import { computed, onBeforeUnmount, ref } from 'vue'
import { app, partnerOf } from '../lib/app'
import { keyLabel, modsLabel } from '../lib/keycodes'
import type { Session } from '../lib/session'
import { VirtualTransport } from '../lib/transport'
import KeyboardView from './KeyboardView.vue'

const props = defineProps<{ session: Session }>()

const transport = computed(() => props.session.transport as VirtualTransport)
const held = ref(new Set<number>())
const open = ref(true)
const cable = ref(true)
const partner = computed(() => partnerOf(props.session))

function press(key: number, event: PointerEvent) {
  // Shift-click latches a key down so two can be held with one mouse.
  if (event.shiftKey && held.value.has(key)) return release(key)
  held.value.add(key)
  transport.value.setKey(key, true)
  if (!event.shiftKey) window.addEventListener('pointerup', () => release(key), { once: true })
}

function release(key: number) {
  held.value.delete(key)
  transport.value.setKey(key, false)
}

function toggleCable() {
  cable.value = !cable.value
  transport.value.setCable(cable.value)
}

onBeforeUnmount(() => held.value.forEach((key) => transport.value.setKey(key, false)))

// What the computer has received, as the text it would produce where possible.
const typed = computed(() => {
  const out: string[] = []
  let previous: number[] = []
  for (const event of props.session.typed) {
    if ('Consumer' in event) {
      if (event.Consumer) out.push('♪')
      continue
    }
    const fresh = event.Keyboard.keys.filter((k) => !previous.includes(k))
    previous = event.Keyboard.keys
    for (const usage of fresh) {
      const label = keyLabel(usage)
      const mods = event.Keyboard.mods & ~0x22 ? modsLabel(event.Keyboard.mods & ~0x22) + '+' : ''
      const shifted = event.Keyboard.mods & 0x22
      out.push(mods + (label.length === 1 ? (shifted ? label : label.toLowerCase()) : `⟨${label}⟩`))
    }
  }
  return out.slice(-40).join('')
})
</script>

<template>
  <aside class="dock" :class="{ open }" data-test="virtual-board">
    <button class="handle" @click="open = !open">
      <span class="chip signal">virtual</span>
      <strong>{{ session.demo!.label }}</strong>
      <span class="muted hint">click keys to press them · shift-click to hold</span>
      <span class="grow" />
      <span class="muted">{{ open ? 'hide' : 'show' }}</span>
    </button>
    <div v-if="open" class="body">
      <div class="hardware">
        <KeyboardView
          :layout="session.demo!.keys"
          :pressed="held"
          interactive
          :max-unit="44"
          @down="press"
        />
      </div>
      <div class="side col">
        <div v-for="(_, index) in session.demo!.wiring.encoders" :key="index" class="row knob">
          <button class="btn small" :data-test="'knob-ccw-' + index" @click="transport.turnEncoder(index, false)">⟲</button>
          <span class="muted">knob {{ index + 1 }}</span>
          <button class="btn small" :data-test="'knob-cw-' + index" @click="transport.turnEncoder(index, true)">⟳</button>
        </div>
        <div v-if="partner" class="row">
          <button class="btn small" data-test="cable" @click="toggleCable">
            {{ cable ? 'Unplug cable' : 'Plug cable in' }}
          </button>
          <button class="btn small ghost" data-test="other-half" @click="app.currentId = partner.id">
            Go to {{ partner.demo!.label.toLowerCase() }}
          </button>
        </div>
        <div class="screen">
          <span class="muted label">the computer received</span>
          <div class="typed mono" data-test="typed">{{ typed }}<span class="caret" /></div>
        </div>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.dock {
  flex: none;
  border-top: 1px solid var(--line);
  background: var(--panel);
}

.handle {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 18px;
  background: none;
  border: none;
  cursor: pointer;
  text-align: left;
}

.hint {
  font-size: 0.82rem;
}

.body {
  display: flex;
  gap: 24px;
  padding: 4px 18px 16px;
  align-items: center;
}

.hardware {
  flex: 1;
  min-width: 0;
  display: grid;
  place-items: center;
  padding: 10px;
  border-radius: 12px;
  background:
    radial-gradient(circle at 1px 1px, var(--line) 1px, transparent 0) 0 0 / 14px 14px,
    var(--bg);
  max-height: 220px;
}

.hardware :deep(svg) {
  max-height: 200px;
}

.side {
  width: 300px;
  flex: none;
}

.screen {
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 8px 12px;
  background: var(--bg);
}

.label {
  font-size: 0.75rem;
}

.typed {
  min-height: 1.6em;
  word-break: break-all;
}

.caret {
  display: inline-block;
  width: 7px;
  height: 1em;
  margin-left: 1px;
  vertical-align: text-bottom;
  background: var(--signal);
  animation: blink 1s steps(2) infinite;
}

@keyframes blink {
  50% {
    opacity: 0;
  }
}
</style>
