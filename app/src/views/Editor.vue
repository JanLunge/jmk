<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import KeyboardView from '../components/KeyboardView.vue'
import { attempt, partnerOf, toast } from '../lib/app'
import { hasKey } from '../lib/layout'
import { LIMITS } from '../lib/protocol'
import { adoptRemoteKeys, role, save, setMeta, type Session } from '../lib/session'
import BehaviorsTab from './BehaviorsTab.vue'
import HardwareTab from './HardwareTab.vue'
import KeymapTab from './KeymapTab.vue'
import LayoutTab from './LayoutTab.vue'
import ShareTab from './ShareTab.vue'

const props = defineProps<{ session: Session }>()

const isPeripheral = computed(() => role(props.session) === 'peripheral')
const tabs = computed(() =>
  isPeripheral.value
    ? (['hardware'] as const)
    : (['keymap', 'layout', 'behaviours', 'hardware', 'share'] as const),
)
type Tab = 'keymap' | 'layout' | 'behaviours' | 'hardware' | 'share'
const tab = ref<Tab>(isPeripheral.value ? 'hardware' : 'keymap')
const saving = ref(false)
const keyCount = computed(() => props.session.layout.filter(hasKey).length)

async function saveNow() {
  saving.value = true
  if (await attempt(() => save(props.session).then(() => true))) toast('Saved to the keyboard')
  saving.value = false
}

async function rename(event: Event) {
  const name = (event.target as HTMLInputElement).value.trim().slice(0, LIMITS.nameLength)
  if (name !== props.session.meta.name) await attempt(() => setMeta(props.session, { ...props.session.meta, name }))
}

// A split's central half learns about the other half over the cable.
let timer: ReturnType<typeof setInterval> | undefined
async function checkSplit() {
  if (role(props.session) === 'single' || props.session.closed) return
  try {
    props.session.split = await props.session.client.splitStatus()
    if (await adoptRemoteKeys(props.session, partnerOf(props.session))) toast("Added the other half's keys")
  } catch {
    // Unplugged mid-request; the session is marked closed elsewhere.
  }
}
onMounted(() => {
  checkSplit()
  timer = setInterval(checkSplit, 1500)
})
onBeforeUnmount(() => clearInterval(timer))
</script>

<template>
  <div class="editor">
    <header class="bar row">
      <input class="name" :value="session.meta.name" placeholder="My keyboard" :maxlength="LIMITS.nameLength" data-test="rename" @change="rename" />
      <span class="chip">{{ keyCount }} keys</span>
      <span v-if="role(session) !== 'single'" class="chip" :class="session.split.connected ? 'signal' : ''" data-test="split-status">
        {{ role(session) === 'central' ? 'main half' : 'other half' }} ·
        {{ session.split.connected ? 'halves connected' : 'other half not heard' }}
      </span>
      <span class="grow" />
      <Transition name="fade">
        <span v-if="session.unsaved" class="muted small">Changes are live but not saved yet</span>
      </Transition>
      <button class="btn" :class="{ primary: session.unsaved, pulse: session.unsaved }" :disabled="saving || !session.unsaved" data-test="save" @click="saveNow">
        {{ session.unsaved ? 'Save to keyboard' : 'Saved' }}
      </button>
    </header>

    <nav class="tabs row">
      <button v-for="t in tabs" :key="t" class="tab" :class="{ on: tab === t }" :data-test="'nav-' + t" @click="tab = t">{{ t }}</button>
    </nav>

    <div class="content">
      <Transition name="rise" mode="out-in">
        <KeymapTab v-if="tab === 'keymap'" :session="session" />
        <LayoutTab v-else-if="tab === 'layout'" :session="session" />
        <BehaviorsTab v-else-if="tab === 'behaviours'" :session="session" />
        <ShareTab v-else-if="tab === 'share'" :session="session" />
        <div v-else class="hardware-wrap scroll">
          <div v-if="isPeripheral" class="card col peripheral">
            <h2>This is the other half</h2>
            <p class="muted">
              It reports its keys over the cable. Open the half that plugs into the computer to change what the
              keys do. Press keys here to check they are seen.
            </p>
            <KeyboardView :layout="session.layout" :pressed="session.pressed" data-test="peripheral-tester" />
          </div>
          <HardwareTab :session="session" />
        </div>
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.editor {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.bar {
  padding: 14px 24px 8px;
  gap: 12px;
}

.name {
  font-size: 1.3rem;
  font-weight: 700;
  letter-spacing: -0.01em;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 8px;
  padding: 2px 8px;
  margin-left: -8px;
  width: 260px;
  outline: none;
  transition: border-color 0.15s;
}

.name:hover,
.name:focus {
  border-color: var(--line);
}

.small {
  font-size: 0.85rem;
}

.tabs {
  padding: 0 24px;
  gap: 4px;
  border-bottom: 1px solid var(--line);
}

.tab {
  padding: 9px 14px;
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  color: var(--muted);
  text-transform: capitalize;
  cursor: pointer;
  transition: color 0.15s;
}

.tab:hover {
  color: var(--text);
}

.tab.on {
  color: var(--text);
  border-bottom-color: var(--accent);
}

.content {
  flex: 1;
  min-height: 0;
}

.content > * {
  height: 100%;
}

.hardware-wrap {
  padding: 18px 24px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.peripheral {
  align-items: flex-start;
}

.pulse {
  animation: nudge 2.4s ease-in-out infinite;
}

@keyframes nudge {
  50% {
    box-shadow: 0 0 0 6px var(--accent-soft);
  }
}
</style>
