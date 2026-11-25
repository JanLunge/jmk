<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import KeyboardView from '../components/KeyboardView.vue'
import TryBox from '../components/TryBox.vue'
import { attempt, cabled, pairByCable, partnerOf, reconcile, resolveConflict, saveAll, sideOf, toast } from '../lib/app'
import { carriedVersion, updateDue, updateFirmware, updating } from '../lib/firmware'
import { historyOf, redo, startHistory, undo } from '../lib/history'
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

const keyCount = computed(() => props.session.layout.filter(hasKey).length - knobPositions(props.session).filter((pos) => hasKey(props.session.layout[pos])).length)

const isHalf = computed(() => role(props.session) !== 'single')
const other = computed(() => (sideOf(props.session) === 'left' ? 'right' : 'left'))
const partnerOpen = computed(() => !!partner.value && !partner.value.closed)
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

// Every change is written to the keyboard's flash by itself, a moment after
// the last one, so a burst of changes is one write.
const SAVE_AFTER_MS = 700
let saveTimer: ReturnType<typeof setTimeout> | undefined
const saveFailed = ref(false)
function saveSoon() {
  clearTimeout(saveTimer)
  saveTimer = setTimeout(saveNow, SAVE_AFTER_MS)
}
async function saveNow() {
  clearTimeout(saveTimer)
  saveTimer = undefined
  const { session } = props
  if (session.closed || session.setup) return
  // One save at a time; the one running looks again when it is done.
  if (saving.value) return
  saving.value = true
  const upTo = session.edits
  const done = await attempt(async () => {
    await saveAll(session)
    await noteSaved()
    return true
  })
  saveFailed.value = !done
  saving.value = false
  if (done && session.edits !== upTo) {
    session.unsaved = true
    saveSoon()
  }
}
watch(() => props.session.edits, saveSoon)

/** After a save this half's keymap is the one this editor shows. */
async function noteSaved() {
  if (!isHalf.value) return
  const status = await props.session.client.splitStatus()
  props.session.split = status
  shownDigest = status.digest
  // The other half fetches it when it is back; say so then.
  if (!status.connected) owed = true
}

const saveKind = computed(() => (saveFailed.value ? 'failed' : saving.value || props.session.unsaved ? 'saving' : 'saved'))
const saveText = computed(() => ({ failed: 'Not saved · retry', saving: 'Saving…', saved: 'Saved' })[saveKind.value])

/** On older firmware a changed data pin or cable setting only counts after a restart. */
const restartNeeded = computed(() => [props.session, partner.value].some((half) => half && !half.closed && needsRestart(half)))
/** A half runs older firmware than the app carries. */
const outdated = computed(() => [props.session, partner.value].find((half) => half && updateDue(half)))
const updateHint = computed(() => `Version ${outdated.value?.info.firmware.join('.')} to ${carriedVersion().join('.')}. The keyboard is away for a few seconds and comes back by itself; your settings are kept.`)
const restartingNow = ref(false)
async function restartNow() {
  restartingNow.value = true
  for (const half of [props.session, partner.value]) {
    if (half && !half.closed && needsRestart(half)) await attempt(() => restart(half))
  }
  restartingNow.value = false
}

// --- Undo and redo -----------------------------------------------------------
const history = computed(() => historyOf(props.session))
async function stepBack(which: 'undo' | 'redo') {
  const moved = await attempt(() => (which === 'undo' ? undo : redo)(props.session))
  if (moved) toast(which === 'undo' ? 'Undone' : 'Redone')
}
function onShortcut(event: KeyboardEvent) {
  if (!(event.metaKey || event.ctrlKey) || event.altKey) return
  const key = event.key.toLowerCase()
  if (key !== 'z' && key !== 'y') return
  // Text being typed has its own undo.
  if ((event.target as HTMLElement).closest?.('input, textarea, select, [contenteditable]')) return
  event.preventDefault()
  stepBack(key === 'y' || event.shiftKey ? 'redo' : 'undo')
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
  if (!isHalf.value || props.session.closed || busy.value || saving.value) return
  busy.value = true
  try {
    const status = await props.session.client.splitStatus()
    props.session.split = status
    pairByCable(props.session)
    if (props.session.unsaved || saving.value || shownDigest === undefined) shownDigest = status.digest
    else if (status.digest !== shownDigest) {
      // This half fetched the other half's keymap over the cable.
      await refresh(props.session)
      shownDigest = status.digest
      toast(`The ${sideOf(props.session)} half caught up with the ${other.value} one`)
    }
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

onMounted(async () => {
  // The other half's editor changed the keymap they share.
  if (props.session.stale) {
    await attempt(() => refresh(props.session))
    props.session.stale = false
  }
  startHistory(props.session)
  window.addEventListener('keydown', onShortcut)
  if (props.session.unsaved) saveSoon()
  checkSplit()
  timer = setInterval(checkSplit, 1000)
})
onBeforeUnmount(() => {
  clearInterval(timer)
  window.removeEventListener('keydown', onShortcut)
  // Leaving with a change still waiting: write it now.
  if (saveTimer) saveNow()
})
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
      <TryBox v-if="!session.demo" />
      <button v-if="outdated" class="btn small primary" :disabled="!!updating" :title="updateHint" data-test="firmware-due" @click="attempt(() => updateFirmware(session))">
        Update firmware
      </button>
      <button v-else-if="restartNeeded" class="btn small primary" :disabled="restartingNow || session.unsaved" title="The new pin setting takes effect when the keyboard restarts" data-test="restart" @click="restartNow">
        Restart to apply
      </button>
      <!-- What became of the changes, in one place: step back, step forward, and whether the keyboard has them. -->
      <div class="changes row">
        <button class="step" :disabled="!history.undo || history.busy" title="Undo (Ctrl/⌘ Z). Only what was changed since the app opened." aria-label="Undo" data-test="undo" @click="stepBack('undo')">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 14 4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" /></svg>
        </button>
        <button class="step" :disabled="!history.redo || history.busy" title="Redo (Ctrl/⌘ Shift Z)" aria-label="Redo" data-test="redo" @click="stepBack('redo')">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 14 5-5-5-5" /><path d="M20 9H9.5a5.5 5.5 0 0 0 0 11H13" /></svg>
        </button>
        <button class="save-state" :class="saveKind" :disabled="!saveFailed" :title="saveFailed ? 'Try to save again' : 'Changes are saved to the keyboard by themselves'" data-test="save" @click="saveNow">
          <span class="dot" aria-hidden="true" />{{ saveText }}
        </button>
      </div>
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
  </div>
</template>

<style scoped>
.changes {
  gap: 0;
  align-items: stretch;
  height: 34px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel);
  overflow: hidden;
}

.step,
.save-state {
  border: 0;
  background: none;
  font: inherit;
  color: inherit;
}

.step {
  width: 36px;
  display: grid;
  place-items: center;
  cursor: pointer;
  transition: background 0.15s;
}

.step svg {
  width: 17px;
  height: 17px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.step:hover:not(:disabled) {
  background: var(--panel-2);
}

.step:disabled {
  color: var(--muted);
  opacity: 0.45;
  cursor: default;
}

.save-state {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 13px 0 12px;
  border-left: 1px solid var(--line);
  font-size: 0.85rem;
  color: var(--muted);
  white-space: nowrap;
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--ok);
}

.save-state.saving .dot {
  width: 10px;
  height: 10px;
  background: none;
  border: 1.5px solid var(--accent);
  border-right-color: transparent;
  animation: turn 0.8s linear infinite;
}

.save-state.failed {
  color: var(--danger);
  cursor: pointer;
}

.save-state.failed:hover {
  background: var(--panel-2);
}

.save-state.failed .dot {
  background: var(--danger);
}

@keyframes turn {
  to {
    transform: rotate(360deg);
  }
}

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

.conflict {
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

.pulse {
  animation: nudge 2.4s ease-in-out infinite;
}

@keyframes nudge {
  50% {
    box-shadow: 0 0 0 6px var(--accent-soft);
  }
}
</style>
