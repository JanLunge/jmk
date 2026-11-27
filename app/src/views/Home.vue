<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { arrivals, installArrival } from '../lib/firmware'
import SplitPicture, { type PictureBoard } from '../components/SplitPicture.vue'
import SetupGuide from '../components/SetupGuide.vue'
import { isConfigured, role } from '../lib/session'
import KeyboardView from '../components/KeyboardView.vue'
import { app, connectUsb, openDemo, reconnectUsb } from '../lib/app'
import { DEMOS } from '../lib/demos'
import { appUpdate, desktop, type KeyboardAccess } from '../lib/desktop'
import { usbSupported } from '../lib/transport'

const canUsb = usbSupported()

// Linux keeps programs from a keyboard's settings until a udev rule lets
// them in. The desktop app sees a keyboard it is kept from and can install
// the rule; a browser cannot tell, so there it is a line of advice.
const access = ref<KeyboardAccess>()
const allowing = ref(false)
const allowProblem = ref('')
const byHand = ref(false)
const linuxBrowser = !desktop && canUsb && /Linux/.test(navigator.userAgent) && !/Android/.test(navigator.userAgent)
async function lookAtAccess() {
  access.value = await desktop!.access().catch(() => undefined)
}
if (desktop?.platform === 'linux') {
  lookAtAccess()
  const timer = setInterval(() => allowing.value || lookAtAccess(), 2000)
  onBeforeUnmount(() => clearInterval(timer))
}
async function allow() {
  allowing.value = true
  allowProblem.value = ''
  try {
    const problem = await desktop!.allowAccess()
    if (problem && problem !== 'dismissed') {
      allowProblem.value = problem
      byHand.value = true
    }
    await lookAtAccess()
    if (!access.value?.blocked) await reconnectUsb()
  } finally {
    allowing.value = false
  }
}
const copied = ref(false)
async function copyCommand() {
  await navigator.clipboard.writeText(access.value?.command ?? '')
  copied.value = true
  setTimeout(() => (copied.value = false), 1500)
}

// Putting JMK on a board for the first time (desktop app only).
const installing = ref<'idle' | 'waiting' | 'done'>('idle')
const installError = ref('')
/**
 * Boards seen without firmware since this screen opened. They stay in the
 * picture while they restart as keyboards, so nothing blinks out between
 * the firmware going on and the setup opening.
 */
const seen = reactive(new Map<string, string>())
watch(
  arrivals,
  (now) => {
    now.forEach((arrival) => seen.set(arrival.key, arrival.model))
    // Unplugged again before anything was installed.
    if (installing.value === 'idle') for (const key of [...seen.keys()]) if (!now.some((arrival) => arrival.key === key)) seen.delete(key)
  },
  { immediate: true },
)

async function install() {
  installing.value = 'waiting'
  installError.value = ''
  const before = new Set(app.sessions.map((session) => session.id))
  // Two new boards at once are most likely the halves of one keyboard:
  // their setup opens once both are ready, not after the first.
  app.holding = true
  try {
    await Promise.all(arrivals.value.map(installArrival))
    // The boards restart as JMK keyboards and open by themselves.
    installing.value = 'done'
  } catch (error) {
    installing.value = 'idle'
    installError.value = error instanceof Error ? error.message : String(error)
  }
  // The boards come back one by one, seconds after the copy. Two that were
  // flashed together are most likely the halves of one keyboard: wait for
  // both, so their setup opens once and as a pair.
  const expected = seen.size
  const back = () => app.sessions.filter((session) => !before.has(session.id) && !session.closed)
  for (let waited = 0; installing.value === 'done' && back().length < expected && waited < 15_000; waited += 100) {
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
  app.holding = false
  if (!app.currentId && back().length) app.currentId = back()[0].id
}

/** What the top of the page is about, decided by what is plugged in. */
const stage = computed(() => (!seen.size ? 'waiting' : installing.value === 'done' && !arrivals.value.length ? 'installed' : 'install'))
const arriving = computed<PictureBoard[]>(() =>
  [...seen.entries()].slice(0, 2).map(([key, model], index) => {
    const arrival = arrivals.value.find((each) => each.key === key)
    const label = seen.size > 1 ? (index ? 'second board' : 'first board') : model
    if (!arrival) return { label, state: 'installing', progress: 1, note: 'restarting as a keyboard…' }
    return { label, state: arrival.progress !== undefined || installing.value === 'waiting' ? 'installing' : 'new', progress: arrival.progress }
  }),
)
const helping = ref(false)
/** The list of virtual keyboards is open. */
const trying = ref(false)
const steps = computed(() => (seen.size > 1 ? ['Plug in', 'Firmware', 'Find the cable', 'Find the keys'] : ['Plug in', 'Firmware', 'Find the keys']))
/** One line on what an open keyboard is, so it is clear the app has seen it. */
function describe(session: (typeof app.sessions)[number]): string {
  if (session.closed) return session.restarting ? 'restarting…' : 'unplugged'
  if (!isConfigured(session)) return 'new, not set up yet'
  const kind = role(session) === 'single' ? '' : 'half of a split, '
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
  <div class="home">
    <SetupGuide :steps="steps" :current="stage === 'waiting' ? 0 : 1" />
    <div class="side col">
      <div class="setup-fill" :class="{ listed: app.sessions.length }">
        <Transition name="rise">
          <div v-if="access?.blocked" class="note warn access col" data-test="linux-access">
            <div class="row">
              <div class="col grow">
                <strong>Linux keeps POG from your keyboard.</strong>
                <span class="small">Allow it once with your password.</span>
              </div>
              <button class="btn primary" :disabled="allowing" data-test="linux-allow" @click="allow">{{ allowing ? 'Waiting…' : 'Allow' }}</button>
            </div>
            <span v-if="allowProblem" class="small" data-test="linux-problem">{{ allowProblem }} Run this in a terminal instead:</span>
            <button v-else-if="!byHand" class="link small by-hand" @click="byHand = true">Or do it yourself in a terminal</button>
            <div v-if="byHand" class="row command">
              <code class="mono small grow" data-test="linux-command">{{ access.command }}</code>
              <button class="btn small ghost" @click="copyCommand">{{ copied ? 'Copied' : 'Copy' }}</button>
            </div>
          </div>
        </Transition>
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

        <!-- One thing to do at a time, chosen by what is plugged in. The picture stays and changes with it. -->
        <section class="setup-stage" :class="{ quiet: app.sessions.length }">
          <Transition name="words" mode="out-in">
            <div :key="stage + (installing === 'waiting') + seen.size" class="words" :data-test="stage === 'install' ? 'found-boot' : undefined">
              <template v-if="stage === 'install'">
                <h1>{{ installing === 'waiting' ? 'Installing the firmware…' : 'Install the firmware.' }}</h1>
                <p class="muted lead">
                  {{ seen.size > 1 ? 'Two boards without firmware are connected.' : 'A board without firmware is connected.' }}
                  Copying it takes a few seconds.
                </p>
              </template>
              <template v-else-if="stage === 'installed'">
                <h1>Installed.</h1>
                <p class="muted lead">{{ seen.size > 1 ? 'The boards are' : 'The board is' }} restarting. The setup opens in a moment…</p>
              </template>
              <template v-else>
                <h1>{{ app.sessions.length ? 'Plug in another keyboard.' : 'Plug in your keyboard.' }}</h1>
                <p class="muted lead">
                  <template v-if="desktop">It shows up here by itself.</template>
                  <template v-else>Then connect it below.</template>
                  One board or two: for a split, give each half its own USB cable, just for the setup, and join
                  the halves with their cable first. Plugging that cable in under power shorts its contacts.
                </p>
              </template>
            </div>
          </Transition>
          <SplitPicture :boards="arriving" cable="idle" />
          <div class="below col">
            <p v-if="installError" class="note warn">{{ installError }}</p>
            <button v-if="stage === 'waiting'" class="btn small ghost help" data-test="boot-help" @click="helping = !helping">
              {{ helping ? 'Hide' : 'It does not show up?' }}
            </button>
            <Transition name="rise">
              <ul v-if="helping && stage === 'waiting'" class="help-list muted small">
                <li>A board that runs JMK already opens by itself: new ones in the setup, the others where you left them.</li>
                <li>
                  A new board, or one with other firmware: unplug it completely (for a split, the cable between the
                  halves too), hold BOOT or BOOTSEL, plug in the USB cable, let go.
                  <template v-if="!desktop">A drive appears; copy the JMK firmware file onto it. The desktop app does this for you.</template>
                </li>
                <li>Try another USB cable: many carry power only.</li>
                <li>Boards with a single button (the 0xCB Helios, for one): plug in, hold the button for a full second, let go.</li>
                <li>A split keyboard has two boards. Do this for each half, each on its own USB cable.</li>
              </ul>
            </Transition>
          </div>
        </section>
      </div>

      <footer class="setup-bar row">
        <template v-if="stage !== 'waiting'">
          <span class="muted small">{{ seen.size > 1 ? seen.size + ' × ' : '' }}{{ [...seen.values()][0] }}</span>
          <span class="grow" />
          <span v-if="stage === 'installed'" class="muted small">continues by itself</span>
          <button v-else class="btn primary" :disabled="installing === 'waiting' || !arrivals.length" data-test="install-found" @click="install()">
            {{ installing === 'waiting' ? 'Copying…' : seen.size > 1 ? 'Install on both' : 'Install' }}
          </button>
        </template>
        <template v-else>
          <span class="muted small">No board at hand?</span>
          <div class="menu-wrap">
            <button class="btn ghost" :class="{ on: trying }" data-test="demos" @click="trying = !trying">Try a virtual keyboard ▾</button>
            <Transition name="rise">
              <div v-if="trying" class="menu card col" data-test="demo-list">
                <span class="muted small">The real firmware, running inside the app. Click its keys as if you pressed them.</span>
                <div class="demos">
                  <button
                    v-for="{ demo, layout } in previews"
                    :key="demo.id"
                    class="demo"
                    :title="demo.blurb"
                    :data-test="'demo-' + demo.id"
                    @click="(trying = false), openDemo(demo)"
                  >
                    <div class="preview"><KeyboardView :layout="layout" :max-unit="14" /></div>
                    <span>{{ demo.name }}</span>
                  </button>
                </div>
              </div>
            </Transition>
          </div>
          <span class="grow" />
          <span class="muted small brand" data-test="brand">
            <strong>POG</strong> {{ appUpdate ? appUpdate.version : 'v2' }} · runs on JMK firmware
          </span>
          <button
            v-if="appUpdate && appUpdate.kind !== 'off' && appUpdate.kind !== 'ready'"
            class="btn ghost small"
            :disabled="appUpdate.kind === 'checking' || appUpdate.kind === 'downloading'"
            :title="appUpdate.kind === 'failed' ? appUpdate.message : undefined"
            data-test="check-update"
            @click="desktop?.update.check()"
          >
            {{
              appUpdate.kind === 'checking'
                ? 'Checking…'
                : appUpdate.kind === 'downloading'
                  ? `Downloading ${appUpdate.next}…`
                  : appUpdate.kind === 'current'
                    ? 'Up to date · check again'
                    : appUpdate.kind === 'failed'
                      ? 'Could not check · try again'
                      : 'Check for updates'
            }}
          </button>
          <button v-if="canUsb" class="btn" :class="{ primary: !desktop }" data-test="connect" @click="connectUsb">
            {{ desktop ? 'Look for a keyboard' : 'Connect a keyboard' }}
          </button>
          <span v-else class="muted small">This browser cannot talk to USB keyboards. Use the desktop app.</span>
          <a v-if="linuxBrowser" class="muted small" href="https://github.com/JanLunge/jmk#linux" target="_blank" rel="noopener" data-test="linux-hint">
            Not found on Linux?
          </a>
        </template>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.side {
  flex: 1;
  min-height: 0;
}

.listed {
  gap: 32px;
}

.setup-stage.quiet h1 {
  font-size: 1.5rem;
}

.menu-wrap {
  position: relative;
}

.menu {
  position: absolute;
  bottom: calc(100% + 8px);
  left: 0;
  width: min(560px, 80vw);
  padding: 12px;
  gap: 10px;
  z-index: 5;
  box-shadow: var(--shadow);
}

.home {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.access {
  gap: 8px;
  margin-bottom: 20px;
  align-self: center;
  width: 100%;
  max-width: 640px;
}

.access .row {
  gap: 14px;
  align-items: center;
}

.access .by-hand {
  align-self: flex-start;
  padding: 0;
  border: 0;
  background: none;
  color: inherit;
  text-decoration: underline;
  cursor: pointer;
}

.access code {
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--panel);
  overflow-wrap: anywhere;
  user-select: all;
}

.open {
  gap: 12px;
  align-self: center;
  width: 100%;
  max-width: 640px;
  animation: arrive 0.4s var(--ease) backwards;
}

/* The stage takes whatever height is left, so the one thing to do sits in the middle of the window. */

.centre {
  align-items: center;
  text-align: center;
  gap: 14px;
  max-width: 720px;
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
