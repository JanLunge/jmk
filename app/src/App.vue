<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import VirtualBoard from './components/VirtualBoard.vue'
import { app, benchOf, closeSession, current, halvesOf, partnerOf, reconnectUsb } from './lib/app'
import { desktop } from './lib/desktop'
import { watchBootDrives } from './lib/firmware'
import { role, type Session } from './lib/session'
import Editor from './views/Editor.vue'
import Home from './views/Home.vue'
import Wizard from './views/Wizard.vue'

/** The keyboards that are open: the two halves of a split are one keyboard. */
const keyboards = computed(() => {
  const seen = new Set<string>()
  const list: Session[][] = []
  for (const session of app.sessions) {
    if (seen.has(session.id)) continue
    const halves = halvesOf(session)
    halves.forEach((half) => seen.add(half.id))
    list.push(halves)
  }
  return list
})
const nameOf = (halves: Session[]) => halves.find((half) => half.meta.name)?.meta.name || (halves.length > 1 ? 'Split keyboard' : halves[0].transport.label)
/** Bring a keyboard to the front: the half that is in front already, else one that is plugged in. */
function show(halves: Session[]) {
  if (halves.some((half) => half.id === app.currentId)) return
  app.currentId = (halves.find((half) => !half.closed) ?? halves[0]).id
}

// A half whose USB cable was pulled hands the screen to the other half, so
// the keyboard stays open as long as one of them is reachable.
watch(
  () => [current.value?.closed, current.value && partnerOf(current.value)?.closed],
  () => {
    const now = current.value
    const partner = now && partnerOf(now)
    if (now?.closed && !now.setup && !now.restarting && partner && !partner.closed && !partner.setup) app.currentId = partner.id
  },
)

// The first screen and the wizard are one setup: going between them is a
// fade in place, so what they share stays put. Anything else is a new page.
const kind = computed(() => (!current.value ? 'home' : current.value.closed ? 'gone' : current.value.setup ? 'wizard' : 'editor'))
const blending = ref(false)
watch(
  [kind, () => app.currentId],
  ([now], [before]) => (blending.value = now !== before && [now, before].includes('home') && [now, before].includes('wizard')),
  { flush: 'sync' },
)

/** The virtual workbench shown below: the keyboard in front, or boards that are not open yet. */
const bench = computed(() => {
  if (current.value) return benchOf(current.value) ?? app.benches.find((each) => each.halves.some((half) => half.sessionId === current.value!.id))
  return app.benches.find((each) => each.halves.some((half) => !half.sessionId))
})

onMounted(() => {
  reconnectUsb()
  watchBootDrives()
  // A keyboard plugged in later (or back from a firmware update) opens by itself.
  if ('hid' in navigator) navigator.hid.addEventListener('connect', reconnectUsb)
})
</script>

<template>
  <div class="shell" :class="{ desktop }">
    <!-- The desktop window has no system title bar; this is it. -->
    <header v-if="desktop" class="titlebar" :class="'on-' + desktop.platform" data-test="titlebar">
      <span class="title">
        <strong>POG</strong><span class="version" title="The second POG, built on JMK firmware">v2</span>
        <template v-if="current"> · {{ current.meta.name || current.transport.label }}</template>
        <span v-if="current?.unsaved" class="unsaved" title="Unsaved changes" />
      </span>
    </header>
    <div class="frame">
    <nav class="rail">
      <button class="logo" :class="{ active: !current }" title="Home" @click="app.currentId = undefined">
        <svg viewBox="0 0 32 32" width="26" height="26" aria-hidden="true">
          <rect x="3" y="8" width="26" height="16" rx="4" fill="none" stroke="currentColor" stroke-width="2.2" />
          <rect v-for="i in 4" :key="i" :x="1 + i * 5.2" y="12" width="3.2" height="3.2" rx="1" fill="currentColor" />
          <rect x="10" y="18" width="12" height="3" rx="1.5" fill="currentColor" />
        </svg>
      </button>
      <TransitionGroup name="pop" tag="div" class="boards">
        <button
          v-for="halves in keyboards"
          :key="halves[0].id"
          class="board"
          :class="{ active: halves.some((half) => half.id === app.currentId), closed: halves.every((half) => half.closed) }"
          :title="nameOf(halves)"
          data-test="rail-keyboard"
          @click="show(halves)"
        >
          <span v-if="halves.length === 1" class="initial">{{ nameOf(halves).slice(0, 1).toUpperCase() }}</span>
          <span v-else class="pair" title="Two halves, one keyboard">
            <i v-for="half in halves" :key="half.id" :class="{ off: half.closed }" />
          </span>
          <span v-if="halves[0].transport.kind === 'virtual'" class="badge" title="Virtual keyboard">v</span>
          <span v-if="halves.some((half) => half.unsaved)" class="dot" title="Unsaved changes" />
        </button>
      </TransitionGroup>
    </nav>

    <main class="main">
      <div class="view">
      <Transition :name="blending ? 'blend' : 'rise'" :mode="blending ? undefined : 'out-in'">
        <Home v-if="!current" key="home" />
        <div v-else-if="current.closed" :key="current.id + 'gone'" class="gone col">
          <template v-if="current.restarting">
            <h2>The keyboard is restarting…</h2>
            <p class="muted">It comes back here by itself in a few seconds. Leave it plugged in.</p>
            <button class="btn ghost" @click="closeSession(current)">Close</button>
          </template>
          <template v-else>
            <h2>This keyboard was unplugged</h2>
            <p class="muted">
              Plug it back in and it opens here again by itself<template v-if="current.setup">, with the keys you pressed so far</template>.
            </p>
            <button class="btn" @click="closeSession(current)">Close</button>
          </template>
        </div>
        <Wizard v-else-if="current.setup" :key="current.id + 'wizard'" :session="current" />
        <Editor v-else :key="current.id + role(current)" :session="current" />
      </Transition>
      </div>
      <Transition name="rise" mode="out-in">
        <VirtualBoard v-if="bench" :key="bench.id" :bench="bench" :setup="!current || current.closed || current.setup" />
      </Transition>
    </main>
    </div>

    <TransitionGroup name="pop" tag="div" class="toasts">
      <div v-for="toast in app.toasts" :key="toast.id" class="toast" :class="toast.kind">{{ toast.text }}</div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.frame {
  flex: 1;
  min-height: 0;
  display: flex;
}

/* Keep the height in step with TITLEBAR_HEIGHT in electron/main.cjs. */
.titlebar {
  flex: none;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  /* Room for the window buttons: left on macOS, right elsewhere. */
  padding: 0 140px;
  background: var(--panel);
  border-bottom: 1px solid var(--line);
  user-select: none;
  -webkit-app-region: drag;
}

.titlebar.on-darwin {
  padding: 0 84px;
}

.title {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 0.87rem;
  color: var(--muted);
}

.title strong {
  color: var(--text);
  letter-spacing: 0.04em;
}

.version {
  padding: 0 5px;
  border-radius: 5px;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 0.72rem;
  font-weight: 600;
  line-height: 1.5;
}

.unsaved {
  flex: none;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--accent);
}

.desktop .toasts {
  top: 52px;
}

.rail {
  width: 64px;
  flex: none;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
  padding: 14px 0;
  border-right: 1px solid var(--line);
  background: var(--panel);
}

.boards {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.logo,
.board {
  position: relative;
  width: 42px;
  height: 42px;
  display: grid;
  place-items: center;
  border-radius: 13px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s,
    border-radius 0.25s var(--ease),
    transform 0.15s var(--ease);
}

.board {
  background: var(--panel-2);
  border-color: var(--line);
  font-weight: 700;
}

.logo:hover,
.board:hover {
  color: var(--text);
  transform: scale(1.06);
}

.pair {
  display: flex;
  gap: 3px;
}

.pair i {
  width: 9px;
  height: 14px;
  border-radius: 3px;
  background: currentColor;
}

.pair i.off {
  background: none;
  border: 1.5px solid currentColor;
  opacity: 0.6;
}

.logo.active {
  color: var(--accent);
}

.board.active {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-ink);
  border-radius: 16px 16px 16px 5px;
}

.board.closed {
  opacity: 0.4;
}

.badge {
  position: absolute;
  right: -4px;
  bottom: -4px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--signal);
  color: #06201d;
  font-size: 10px;
  line-height: 16px;
}

.dot {
  position: absolute;
  right: -3px;
  top: -3px;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--accent);
  border: 2px solid var(--panel);
}

.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.view {
  flex: 1;
  min-height: 0;
  position: relative;
  display: flex;
  flex-direction: column;
}

.view > * {
  flex: 1;
  min-height: 0;
}

.gone {
  align-items: center;
  justify-content: center;
}

.toasts {
  position: fixed;
  left: 50%;
  top: 14px;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  z-index: 50;
}

.toast {
  padding: 10px 16px;
  border-radius: 10px;
  background: var(--panel-2);
  border: 1px solid var(--line);
  box-shadow: var(--shadow);
  max-width: 380px;
}

.toast.error {
  border-color: var(--danger);
}

.toast.ok {
  border-color: var(--ok);
}
</style>
