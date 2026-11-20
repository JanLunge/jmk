<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import KeyboardView from '../components/KeyboardView.vue'
import ScratchPad from '../components/ScratchPad.vue'
import { app, attempt, cabled, pairByCable, partnerOf, reconcile, resolveConflict, saveAll, sideOf, toast } from '../lib/app'
import { legend } from '../lib/keycodes'
import { hasKey } from '../lib/layout'
import { LIMITS } from '../lib/protocol'
import { knobPositions, needsRestart, refresh, restart, role, setMeta, type Session } from '../lib/session'
import BehaviorsTab from './BehaviorsTab.vue'
import HardwareTab from './HardwareTab.vue'
import KeymapTab from './KeymapTab.vue'
import LayoutTab from './LayoutTab.vue'
import LightingTab from './LightingTab.vue'
import ShareTab from './ShareTab.vue'

const props = defineProps<{ session: Session }>()

const tabs = ['keymap', 'layout', 'behaviours', 'lighting', 'hardware', 'share'] as const
type Tab = (typeof tabs)[number]
const tab = ref<Tab>('keymap')
const partner = computed(() => partnerOf(props.session))
const saving = ref(false)

// The scratch pad stays the way it was left, open or closed.
const SCRATCH_KEY = 'pog.scratch'
const scratch = ref(localStorage.getItem(SCRATCH_KEY) === 'open')
watch(scratch, (open) => localStorage.setItem(SCRATCH_KEY, open ? 'open' : 'closed'))
const keyCount = computed(() => props.session.layout.filter(hasKey).length - knobPositions(props.session).filter((pos) => hasKey(props.session.layout[pos])).length)

const isHalf = computed(() => role(props.session) !== 'single')
const other = computed(() => (sideOf(props.session) === 'left' ? 'right' : 'left'))
const partnerOpen = computed(() => !!partner.value && !partner.value.closed)
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

async function saveNow() {
  saving.value = true
  if (await attempt(() => saveAll(props.session).then(() => true))) {
    await attempt(reportSave)
    for (const half of [props.session, partner.value]) {
      if (half && !half.closed && needsRestart(half)) {
        toast('Restarting the keyboard so the new pin setting takes effect')
        await attempt(() => restart(half))
      }
    }
  }
  saving.value = false
}

/** Say where a save went. A half reached only through the cable takes a moment to fetch it. */
async function reportSave() {
  if (!isHalf.value) return toast('Saved to the keyboard')
  if (partnerOpen.value && !partner.value!.unsaved) return toast('Saved to both halves')
  for (let tries = 0; tries < 16 && !props.session.closed; tries++) {
    const status = await props.session.client.splitStatus()
    props.session.split = status
    shownDigest = status.digest
    if (!status.connected) {
      owed = true
      return toast(`Saved. The ${other.value} half gets it when it is connected.`)
    }
    if (status.sync === 'Same') return toast('Saved to both halves')
    if (status.sync === 'Conflict') return
    await wait(250)
  }
  toast(`Saved. The ${other.value} half is still fetching it.`)
}

// What is being typed is kept apart from the keyboard's name: the header
// redraws whenever the keyboard reports something, which must not wipe a
// name half typed.
const nameDraft = ref(props.session.meta.name)
const naming = ref(false)
watch(
  () => props.session.meta.name,
  (name) => !naming.value && (nameDraft.value = name),
)
async function rename() {
  naming.value = false
  const name = nameDraft.value.trim().slice(0, LIMITS.nameLength)
  nameDraft.value = name
  if (name === props.session.meta.name) return
  // The name belongs to the whole keyboard.
  for (const half of [props.session, partner.value]) {
    if (half && !half.closed) await attempt(() => setMeta(half, { ...half.meta, name }))
  }
}

// A half hears over the cable who the other half is, which of them is
// typing and how their keymaps compare. The firmware keeps the keymaps the
// same over the cable; two halves that are both on USB but not joined are
// brought level by the app.
let timer: ReturnType<typeof setInterval> | undefined
/** The keymap this editor shows, as the keyboard's digest of it. */
let shownDigest: number | undefined
/** The other half is known to hold an older keymap; say so when it has caught up. */
let owed = false
const conflict = ref(false)
const busy = ref(false)
async function checkSplit() {
  if (!isHalf.value || props.session.closed || busy.value) return
  busy.value = true
  try {
    const status = await props.session.client.splitStatus()
    props.session.split = status
    pairByCable(props.session)
    if (props.session.unsaved || shownDigest === undefined) shownDigest = status.digest
    else if (status.digest !== shownDigest) {
      // This half fetched the other half's keymap over the cable.
      await refresh(props.session)
      shownDigest = status.digest
      toast(`The ${sideOf(props.session)} half caught up with the ${other.value} one`)
    }
    if (status.connected && status.sync === 'Ahead') owed = true
    if (status.connected && status.sync === 'Same' && owed) {
      owed = false
      toast(`The ${other.value} half caught up`)
    }
    let outcome: Awaited<ReturnType<typeof reconcile>> = status.sync === 'Conflict' ? 'conflict' : undefined
    if (partnerOpen.value) outcome = await reconcile(props.session, partner.value!)
    conflict.value = outcome === 'conflict'
    if (outcome && outcome !== 'conflict') {
      if (outcome === props.session) {
        shownDigest = props.session.split.digest
        toast(`The ${sideOf(props.session)} half caught up with the ${other.value} one`)
      } else {
        owed = false
        toast(`The ${other.value} half caught up`)
      }
    }
  } catch {
    // Unplugged mid-request; the session is marked closed elsewhere.
  }
  busy.value = false
}

/** The user's answer to two halves that were both changed while apart. */
async function keep(which: 'this' | 'other') {
  const winner = which === 'this' ? props.session : partner.value
  busy.value = true
  const done = await attempt(async () => {
    if (winner && !winner.closed) await resolveConflict(winner)
    else await props.session.client.resolveSplit(false)
    return true
  })
  busy.value = false
  if (!done) return
  conflict.value = false
  // Whatever this half holds afterwards is what the user chose.
  shownDigest = undefined
  owed = false
  for (let tries = 0; tries < 16; tries++) {
    await wait(250)
    const status = await props.session.client.splitStatus().catch(() => undefined)
    if (!status || status.sync === 'Same' || !status.connected) break
  }
  await attempt(() => refresh(props.session))
  toast('Both halves hold the same keymap now')
}

/** One line on how the two halves are joined and which of them types. */
const joined = computed(() => {
  const { split } = props.session
  const mine = sideOf(props.session)
  if (split.connected) {
    if (partnerOpen.value && cabled(props.session, partner.value!)) return `the ${split.leading ? mine : other.value} half types for both`
    return `${mine} half on USB · ${other.value} half through the cable`
  }
  return partnerOpen.value ? 'halves apart · each types its own keys' : `the ${other.value} half is away`
})
/** The header's word on the halves. */
const pill = computed(() => {
  const { split } = props.session
  if (conflict.value) return { kind: 'warn', text: 'needs you' }
  if (!split.connected) return { kind: 'warn', text: partnerOpen.value ? 'halves apart' : `${other.value} half away` }
  if (split.sync === 'Behind') return { kind: 'signal', text: `${sideOf(props.session)} catching up…` }
  if (split.sync === 'Ahead') return { kind: 'signal', text: `${other.value} catching up…` }
  return { kind: 'signal', text: split.sync === 'Same' ? 'in sync' : 'connected' }
})
const details = ref(false)
/** Everything else about the halves, for the panel under the pill. */
const facts = computed(() => {
  const { split, info } = props.session
  const mine = sideOf(props.session)
  const typing = (side: string) => (!split.connected ? ' · types its own keys' : (split.leading ? mine : other.value) === side ? ' · types for both' : '')
  const where = (side: string) => (side === mine || partnerOpen.value ? 'on USB' : split.connected ? 'through the cable' : 'away')
  const describe = (side: string) => (where(side) === 'away' ? 'away' : where(side) + typing(side))
  const keymap =
    `revision ${split.revision}` +
    (conflict.value ? ', changed on both halves' : !split.connected ? '' : split.sync === 'Same' ? ', same on both' : split.sync === 'Unknown' ? '' : ', being brought level')
  const versions = [info.firmware.join('.'), ...(partnerOpen.value ? [partner.value!.info.firmware.join('.')] : [])]
  return [
    ['Left half', describe('left')],
    ['Right half', describe('right')],
    ['Cable', split.connected ? 'connected' : 'not connected'],
    ['Keymap', keymap],
    ['Keys', String(keyCount.value)],
    ['Firmware', new Set(versions).size > 1 ? versions.join(' and ') : versions[0]],
  ]
})

// --- A conflict, shown as the two keymaps side by side ---------------------------
const legendOf = (half: Session) => (pos: number) => (half.layers[0]?.[pos] ? legend(half.layers[0][pos]) : undefined)
/** Positions whose key differs between the halves on any layer. */
const differing = computed(() => {
  const theirs = partner.value
  if (!conflict.value || !theirs || theirs.closed) return []
  const count = Math.max(props.session.layout.length, theirs.layout.length)
  const layers = Math.max(props.session.layers.length, theirs.layers.length)
  return [...Array(count).keys()].filter((pos) =>
    [...Array(layers).keys()].some((layer) => JSON.stringify(props.session.layers[layer]?.[pos]) !== JSON.stringify(theirs.layers[layer]?.[pos])),
  )
})
const sides = computed(() => [
  { which: 'this' as const, name: sideOf(props.session), half: props.session as Session | undefined, revision: props.session.split.revision },
  {
    which: 'other' as const,
    name: other.value,
    half: partnerOpen.value ? partner.value : undefined,
    revision: partnerOpen.value ? partner.value!.split.revision : props.session.split.peer_revision,
  },
])
// The other half's keymap may have changed since the app last read it.
watch(conflict, async (now) => {
  if (now && partnerOpen.value) await attempt(() => refresh(partner.value!))
})

// --- The guide carries on for a keyboard that was just set up ----------------------
const seenLayout = ref(false)
watch(tab, (now) => now === 'layout' && (seenLayout.value = true))
const coach = computed<{ title: string; text: string; go?: Tab; label?: string } | undefined>(() => {
  if (!app.fresh.includes(props.session.id)) return undefined
  if (!seenLayout.value) {
    return {
      title: 'It types',
      text: 'Every key has a placeholder letter. Next: arrange the keys the way they sit on your desk.',
      go: 'layout',
      label: 'Open Layout',
    }
  }
  if (tab.value === 'layout') return { title: 'Next up', text: 'Drag the keys into place. Then choose what each one does.', go: 'keymap', label: 'Open Keymap' }
  return { title: 'Last step', text: 'Click a key and pick what it does. Changes work at once; Save keeps them on the keyboard.' }
})
function dismissCoach() {
  app.fresh = app.fresh.filter((id) => id !== props.session.id && id !== partner.value?.id)
}

onMounted(async () => {
  // The other half's editor changed the keymap they share.
  if (props.session.stale) {
    await attempt(() => refresh(props.session))
    props.session.stale = false
  }
  checkSplit()
  timer = setInterval(checkSplit, 1000)
})
onBeforeUnmount(() => clearInterval(timer))
</script>

<template>
  <div class="editor">
    <header class="bar row">
      <input
        class="name"
        :class="{ empty: !session.meta.name }"
        v-model="nameDraft"
        placeholder="Name your keyboard"
        :maxlength="LIMITS.nameLength"
        data-test="rename"
        @focus="naming = true"
        @blur="rename"
        @keydown.enter="($event.target as HTMLInputElement).blur()"
      />
      <div v-if="isHalf" class="pill-wrap">
        <!-- One word on how the halves are; the rest opens from it. -->
        <button class="chip halves" :class="pill.kind" :title="joined" :data-revision="session.split.revision" data-test="split-status" @click="details = !details">
          <span class="glyph" aria-hidden="true">
            <i :class="{ usb: sideOf(session) === 'left' || partnerOpen }" /><u :class="{ cut: !session.split.connected }" /><i :class="{ usb: sideOf(session) === 'right' || partnerOpen }" />
          </span>
          {{ pill.text }} <span class="caret-down">▾</span>
        </button>
        <Transition name="rise">
          <dl v-if="details" class="pop card" data-test="split-details" @click="details = false">
            <template v-for="row in facts" :key="row[0]">
              <dt>{{ row[0] }}</dt>
              <dd>{{ row[1] }}</dd>
            </template>
          </dl>
        </Transition>
      </div>
      <span v-else class="muted small" data-test="key-count">{{ keyCount }} keys</span>
      <span class="grow" />
      <button v-if="!session.demo" class="btn" :class="{ on: scratch }" data-test="scratch-toggle" @click="scratch = !scratch">
        <span class="caret" /> Try it
      </button>
      <button class="btn" :class="{ primary: session.unsaved, pulse: session.unsaved }" :disabled="saving || !session.unsaved" data-test="save" @click="saveNow">
        {{ session.unsaved ? 'Save' : 'Saved' }}
      </button>
    </header>

    <nav class="tabs row">
      <button v-for="t in tabs" :key="t" class="tab" :class="{ on: tab === t }" :data-test="'nav-' + t" @click="tab = t">{{ t }}</button>
    </nav>

    <Transition name="rise">
      <div v-if="conflict" class="conflict card col" data-test="halves-conflict">
        <div>
          <strong>Both halves were changed while apart.</strong>&nbsp;
          <span class="muted">
            <template v-if="differing.length">{{ differing.length }} {{ differing.length === 1 ? 'key differs' : 'keys differ' }}.</template>
            Which keymap goes on both?
          </span>
        </div>
        <div class="row sides">
          <div v-for="side in sides" :key="side.which" class="col side">
            <KeyboardView v-if="side.half && partnerOpen" :layout="side.half.layout" :legend="legendOf(side.half)" :selected="differing" :max-unit="26" />
            <div class="row">
              <span class="muted small">{{ side.name }} half · revision {{ side.revision }}</span>
              <button class="btn small" :class="{ primary: side.which === 'this' }" :disabled="busy" :data-test="'keep-' + side.which" @click="keep(side.which)">
                Keep the {{ side.name }} half's
              </button>
            </div>
          </div>
        </div>
      </div>
    </Transition>
    <Transition name="rise">
      <div v-if="coach && !conflict" class="coach row" data-test="coach">
        <b>{{ coach.title }}</b>
        <span>{{ coach.text }}</span>
        <span class="grow" />
        <button v-if="coach.go" class="btn small" data-test="coach-go" @click="tab = coach.go">{{ coach.label }}</button>
        <button class="btn small ghost" title="Hide these hints" data-test="coach-close" @click="dismissCoach">×</button>
      </div>
    </Transition>

    <div class="content">
      <Transition name="rise" mode="out-in">
        <KeymapTab v-if="tab === 'keymap'" :session="session" />
        <LayoutTab v-else-if="tab === 'layout'" :session="session" />
        <BehaviorsTab v-else-if="tab === 'behaviours'" :session="session" />
        <LightingTab v-else-if="tab === 'lighting'" :session="session" />
        <ShareTab v-else-if="tab === 'share'" :session="session" />
        <div v-else class="hardware-wrap scroll">
          <HardwareTab :session="session" />
        </div>
      </Transition>
    </div>
    <Transition name="drawer">
      <ScratchPad v-if="scratch && !session.demo" @close="scratch = false" />
    </Transition>
  </div>
</template>

<style scoped>
.name.empty {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-soft);
}

.pill-wrap {
  position: relative;
}

.halves {
  cursor: pointer;
  font: inherit;
  font-size: 0.8rem;
}

.caret-down {
  opacity: 0.7;
  font-size: 0.7em;
}

.pop {
  position: absolute;
  top: calc(100% + 8px);
  left: 0;
  z-index: 10;
  width: 300px;
  margin: 0;
  padding: 12px 14px;
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 5px 12px;
  box-shadow: var(--shadow);
  font-size: 0.88rem;
}

.pop dt {
  color: var(--muted);
}

.pop dd {
  margin: 0;
}

.conflict,
.coach {
  margin: 12px 24px 0;
  flex: none;
}

.conflict {
  padding: 14px 16px;
  gap: 12px;
}

.sides {
  gap: 28px;
  justify-content: center;
  align-items: flex-end;
  flex-wrap: wrap;
}

.side {
  gap: 8px;
  align-items: center;
}

.coach {
  gap: 12px;
  padding: 8px 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel);
  font-size: 0.9rem;
}

.coach b {
  color: var(--accent);
  font-size: 0.7rem;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  white-space: nowrap;
}

.halves {
  gap: 7px;
}

.halves.warn {
  color: var(--accent);
}

.glyph {
  display: inline-flex;
  align-items: center;
}

.glyph i {
  width: 8px;
  height: 11px;
  border-radius: 2px;
  border: 1.5px solid currentColor;
  opacity: 0.5;
}

.glyph i.usb {
  background: currentColor;
  opacity: 1;
}

.glyph u {
  width: 8px;
  border-top: 1.5px solid currentColor;
}

.glyph u.cut {
  border-top-style: dotted;
  opacity: 0.5;
}

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

.link {
  padding: 0;
  border: none;
  background: none;
  color: inherit;
  font: inherit;
  text-decoration: underline;
  cursor: pointer;
}

.peripheral {
  align-items: flex-start;
}

.btn.on {
  border-color: var(--accent);
  color: var(--accent);
}

.caret {
  width: 2px;
  height: 1em;
  background: currentColor;
  animation: blink 1s steps(2) infinite;
}

@keyframes blink {
  50% {
    opacity: 0;
  }
}

.drawer-enter-active,
.drawer-leave-active {
  transition:
    margin-bottom 0.25s var(--ease),
    opacity 0.2s;
}

.drawer-enter-from,
.drawer-leave-to {
  margin-bottom: -132px;
  opacity: 0;
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
