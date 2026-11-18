<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { bootDrives, installOnBoard } from '../lib/firmware'
import type { BootDrive } from '../lib/desktop'
import { isConfigured, role } from '../lib/session'
import KeyboardView from '../components/KeyboardView.vue'
import { app, connectUsb, openDemo } from '../lib/app'
import { DEMOS } from '../lib/demos'
import { desktop } from '../lib/desktop'
import { usbSupported } from '../lib/transport'

const canUsb = usbSupported()

// Putting JMK on a board for the first time (desktop app only).
const installing = ref<'idle' | 'waiting' | 'done'>('idle')
const installError = ref('')
async function install(drive?: BootDrive) {
  installing.value = 'waiting'
  installError.value = ''
  try {
    await installOnBoard(drive)
    // The board restarts as a JMK keyboard and opens by itself.
    installing.value = 'done'
  } catch (error) {
    installing.value = 'idle'
    installError.value = error instanceof Error ? error.message : String(error)
  }
}
// The next board in update mode is a new one.
watch(bootDrives, (now) => now.length && installing.value === 'done' && (installing.value = 'idle'))

/** What the top of the page is about, decided by what is plugged in. */
const stage = computed(() => (installing.value === 'done' ? 'installed' : bootDrives.value.length ? 'install' : 'waiting'))
const helping = ref(false)

/** One line on what an open keyboard is, so it is clear the app has seen it. */
function describe(session: (typeof app.sessions)[number]): string {
  if (session.closed) return session.restarting ? 'restarting…' : 'unplugged'
  if (!isConfigured(session)) return 'new, not set up yet'
  const kind = { single: '', central: 'main half of a split, ', peripheral: 'other half of a split, ' }[role(session)]
  return `${kind}${session.layout.filter((key) => key.w > 0).length} keys`
}
const serial = (session: (typeof app.sessions)[number]) =>
  session.info.uid.slice(-2).map((byte) => byte.toString(16).padStart(2, '0')).join('')
// Both halves of the split side by side, the right one moved over.
const previews = computed(() =>
  DEMOS.map((demo) => ({
    demo,
    layout: demo.halves.flatMap((half, index) => half.keys.map((key) => ({ ...key, x: key.x + index * 640 }))),
  })),
)
</script>

<template>
  <div class="home scroll">
    <!-- Keyboards that are open already come first: they are what you came back for. -->
    <section v-if="app.sessions.length" class="col open" data-test="found-boards">
      <h2>Your keyboards</h2>
      <TransitionGroup name="pop" tag="div" class="found-list col">
        <button v-for="session in app.sessions" :key="session.id" class="found-row card row" :data-test="'found-' + session.id" @click="app.currentId = session.id">
          <span class="status" :class="{ off: session.closed }" />
          <div class="col">
            <strong>{{ session.meta.name || session.demo?.label || session.transport.label }}</strong>
            <span class="muted small">
              {{ describe(session) }} · firmware {{ session.info.firmware.join('.') }}
              <template v-if="session.transport.kind === 'usb'"> · serial …{{ serial(session) }}</template>
            </span>
          </div>
          <span class="grow" />
          <span class="chip" :class="{ accent: session.setup }">{{ session.setup ? 'Continue setup' : 'Open' }}</span>
        </button>
      </TransitionGroup>
    </section>

    <!-- One thing to do at a time, chosen by what is plugged in. -->
    <section class="stage" :class="{ quiet: app.sessions.length }">
      <Transition name="rise" mode="out-in">
        <div v-if="stage === 'install'" key="install" class="col centre" data-test="found-boot">
          <div class="pulse found"><span class="glyph">＋</span></div>
          <h1>A new board is here.</h1>
          <p class="muted lead">
            {{ bootDrives[0].model }} is in update mode and has no JMK on it yet. Installing takes a few seconds and is
            only needed once.
          </p>
          <button class="btn primary big" :disabled="installing === 'waiting'" data-test="install-found" @click="install(bootDrives[0])">
            {{ installing === 'waiting' ? 'Copying the firmware…' : 'Install JMK on it' }}
          </button>
          <p v-if="installError" class="note warn">{{ installError }}</p>
          <p class="muted small">Afterwards it restarts as a keyboard and the setup opens by itself.</p>
        </div>

        <div v-else-if="stage === 'installed'" key="installed" class="col centre">
          <div class="pulse found"><span class="glyph">✓</span></div>
          <h1>Installed.</h1>
          <p class="muted lead waiting">The board is restarting as a keyboard. Its setup opens in a moment…</p>
        </div>

        <div v-else key="waiting" class="col centre">
          <div class="pulse">
            <svg viewBox="0 0 32 32" width="34" height="34" aria-hidden="true">
              <rect x="3" y="8" width="26" height="16" rx="4" fill="none" stroke="currentColor" stroke-width="2.2" />
              <rect v-for="i in 4" :key="i" :x="1 + i * 5.2" y="12" width="3.2" height="3.2" rx="1" fill="currentColor" />
              <rect x="10" y="18" width="12" height="3" rx="1.5" fill="currentColor" />
            </svg>
          </div>
          <h1>{{ app.sessions.length ? 'Plug in another keyboard.' : 'Plug in your keyboard.' }}</h1>
          <p class="muted lead">
            <template v-if="desktop">It shows up here by itself, and the app takes it from there.</template>
            <template v-else>Then connect it here, and the app takes it from there.</template>
          </p>
          <span v-if="desktop" class="waiting small">Watching the USB ports…</span>

          <div class="paths">
            <div class="path col">
              <span class="tag">Already runs JMK</span>
              <strong>Just plug it in.</strong>
              <p class="muted small">
                <template v-if="desktop">It opens at once: new ones in the setup, the others where you left them.</template>
                <template v-else>Then choose it from the browser's list.</template>
              </p>
              <button class="btn small" :class="{ primary: !desktop }" :disabled="!canUsb" data-test="connect" @click="connectUsb">
                {{ desktop ? 'Not showing up? Look for it' : 'Connect a keyboard' }}
              </button>
              <span v-if="!canUsb" class="muted small">This browser cannot talk to USB keyboards. Use the desktop app.</span>
            </div>
            <div class="path col">
              <span class="tag">New board, or other firmware on it</span>
              <strong>Hold BOOT while plugging it in.</strong>
              <p class="muted small">
                <template v-if="desktop">That puts it in update mode. It appears here and JMK is installed with one click.</template>
                <template v-else>
                  A drive appears; copy the JMK firmware file onto it. The desktop app does this for you.
                </template>
              </p>
              <button class="btn small ghost help" data-test="boot-help" @click="helping = !helping">
                {{ helping ? 'Hide' : 'Nothing appears?' }}
              </button>
            </div>
          </div>
          <Transition name="rise">
            <ul v-if="helping" class="help-list muted small">
              <li>Unplug the board completely first (for a split, the cable between the halves too), then hold BOOT or BOOTSEL, plug in the USB cable, let go.</li>
              <li>Try another USB cable: many carry power only.</li>
              <li>Boards with a single button (the 0xCB Helios, for one): plug in, hold the button for a full second, let go.</li>
              <li>A split keyboard has two boards. Do this for each half, each on its own USB cable.</li>
            </ul>
          </Transition>
        </div>
      </Transition>
    </section>

    <!-- For looking around without hardware. -->
    <section class="virtual col">
      <div class="row">
        <span class="chip">No hardware needed</span>
        <h3>Try a virtual keyboard</h3>
        <span class="muted small">The real firmware, running inside the app. Click its keys as if you pressed them.</span>
        <span class="grow" />
        <span class="muted small brand" data-test="brand"><strong>POG</strong> v2 · runs on JMK firmware</span>
      </div>
      <div class="demos">
        <button
          v-for="({ demo, layout }, index) in previews"
          :key="demo.id"
          class="demo"
          :style="{ animationDelay: 200 + index * 70 + 'ms' }"
          :title="demo.blurb"
          :data-test="'demo-' + demo.id"
          @click="openDemo(demo)"
        >
          <div class="preview"><KeyboardView :layout="layout" :max-unit="14" /></div>
          <span>{{ demo.name }}</span>
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.home {
  min-height: 100%;
  padding: 40px clamp(24px, 6vw, 80px) 28px;
  display: flex;
  flex-direction: column;
  gap: 40px;
}

.open {
  gap: 12px;
  align-self: center;
  width: 100%;
  max-width: 640px;
  animation: arrive 0.4s var(--ease) backwards;
}

/* The stage takes whatever height is left, so the one thing to do sits in the middle of the window. */
.stage {
  flex: 1;
  display: grid;
  place-items: center;
  padding: 24px 0;
}

.centre {
  align-items: center;
  text-align: center;
  gap: 14px;
  max-width: 720px;
}

.centre h1 {
  font-size: 2.3rem;
  margin-top: 10px;
}

.stage.quiet h1 {
  font-size: 1.5rem;
}

.stage.quiet .pulse {
  display: none;
}

.lead {
  font-size: 1.08rem;
  max-width: 520px;
}

.pulse {
  position: relative;
  width: 76px;
  height: 76px;
  border-radius: 50%;
  display: grid;
  place-items: center;
  color: var(--accent);
  background: var(--accent-soft);
}

.pulse::before,
.pulse::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: 50%;
  border: 1.5px solid var(--accent);
  opacity: 0;
  animation: ring 2.6s ease-out infinite;
}

.pulse::after {
  animation-delay: 1.3s;
}

.pulse.found {
  background: var(--accent);
  color: var(--accent-ink);
  animation: land 0.5s cubic-bezier(0.3, 1.6, 0.5, 1);
}

.glyph {
  font-size: 30px;
  font-weight: 700;
  line-height: 1;
}

.big {
  padding: 13px 26px;
  font-size: 1.05rem;
  border-radius: 12px;
  margin-top: 6px;
}

.paths {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0;
  margin-top: 26px;
  width: 100%;
  text-align: left;
}

.path {
  gap: 6px;
  padding: 4px 28px;
  align-items: flex-start;
}

.path + .path {
  border-left: 1px solid var(--line);
}

.path p {
  min-height: 2.6em;
}

.tag {
  font-size: 0.74rem;
  font-weight: 600;
  letter-spacing: 0.07em;
  text-transform: uppercase;
  color: var(--accent);
}

.help {
  margin-left: -10px;
  color: var(--muted);
}

.help-list {
  margin: 4px 0 0;
  padding: 14px 18px 14px 34px;
  text-align: left;
  display: flex;
  flex-direction: column;
  gap: 6px;
  border-radius: 12px;
  background: var(--panel);
  max-width: 620px;
}

.virtual {
  gap: 12px;
  padding-top: 20px;
  border-top: 1px solid var(--line);
}

.virtual .row {
  flex-wrap: wrap;
  gap: 6px 12px;
}

.brand strong {
  color: var(--text);
  letter-spacing: 0.04em;
}

.demos {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}

.demo {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 16px 8px 10px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: none;
  color: var(--muted);
  font-size: 0.9rem;
  cursor: pointer;
  transition:
    transform 0.2s var(--ease),
    border-color 0.2s,
    color 0.2s;
  animation: arrive 0.5s var(--ease) backwards;
}

.demo:hover {
  transform: translateY(-2px);
  border-color: var(--accent);
  color: var(--text);
}

.preview {
  width: 92px;
  height: 40px;
  display: grid;
  place-items: center;
}

.preview :deep(svg) {
  max-height: 40px;
}

.found-list {
  gap: 10px;
}

.found-row {
  gap: 14px;
  padding: 12px 16px;
  text-align: left;
  width: 100%;
  cursor: pointer;
  transition:
    border-color 0.15s,
    transform 0.15s var(--ease);
}

.found-row:hover {
  border-color: var(--accent);
  transform: translateX(3px);
}

.found-row .col {
  gap: 2px;
}

.status {
  flex: none;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--signal);
  box-shadow: 0 0 0 4px var(--signal-soft);
}

.status.off {
  background: var(--muted);
  box-shadow: none;
}

.small {
  font-size: 0.85rem;
}

.waiting {
  color: var(--accent);
  animation: breathe 1.6s ease-in-out infinite;
}

@keyframes arrive {
  from {
    opacity: 0;
    transform: translateY(18px);
  }
}

@keyframes ring {
  0% {
    opacity: 0.55;
    transform: scale(1);
  }
  100% {
    opacity: 0;
    transform: scale(2.1);
  }
}

@keyframes land {
  from {
    transform: scale(0.5);
  }
}

@keyframes breathe {
  50% {
    opacity: 0.45;
  }
}

@media (max-width: 640px) {
  .paths {
    grid-template-columns: 1fr;
    gap: 20px;
  }
  .path + .path {
    border-left: none;
  }
}
</style>
