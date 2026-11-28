// Undo and redo for one keyboard, for as long as the app stays open.
//
// Changes go to the keyboard and its flash by themselves, so there is no
// "revert". Instead the app remembers what the keymap, the picture of the
// keys and the lights were before each change and can write that back. The
// list lives in the app only: another computer that edits the same keyboard
// has its own, and neither can undo the other's work.

import { reactive } from 'vue'
import type { Action, Combo, HoldTapProfile, KeyGeometry, LightingConfig, MacroStep } from './protocol'
import {
  canLight,
  onChange,
  setCombos,
  setLayerColor,
  setLayerWith,
  setLayerCount,
  setLayout,
  setLetterRows,
  setLighting,
  setMacro,
  setMacroName,
  setProfile,
  touch,
  type Session,
} from './session'

/** Everything a step can take back. The wiring and the name are not part of it. */
interface Snapshot {
  layout: KeyGeometry[]
  layers: Action[][]
  layerColors: number[]
  layersWith: number[]
  profiles: HoldTapProfile[]
  combos: Combo[]
  macros: MacroStep[][]
  macroNames: string[]
  letterRows: number[][]
  lighting: LightingConfig
}

interface History {
  /** What the buttons show. */
  state: { undo: number; redo: number; busy: boolean }
  /** The keyboard as of the last finished change; missing while it has to be read afresh. */
  last?: Snapshot
  undo: Snapshot[]
  redo: Snapshot[]
  timer?: ReturnType<typeof setTimeout>
}

/** Changes closer together than this are one step: a slider being dragged, a row of keys assigned at once. */
const SETTLE_MS = 400
const MAX_STEPS = 100

const histories = new WeakMap<Session, History>()

function of(session: Session): History {
  let history = histories.get(session)
  if (!history) {
    history = { state: reactive({ undo: 0, redo: 0, busy: false }), undo: [], redo: [] }
    histories.set(session, history)
  }
  return history
}

const text = (value: unknown) => JSON.stringify(value)

function take(session: Session): Snapshot {
  const { layout, layers, layerColors, layersWith, profiles, combos, macros, macroNames, letterRows, lighting } = session
  return JSON.parse(text({ layout, layers, layerColors, layersWith, profiles, combos, macros, macroNames, letterRows, lighting }))
}

function show(history: History) {
  history.state.undo = history.undo.length
  history.state.redo = history.redo.length
}

/** Forget every step and start from what the keyboard holds now. */
function reset(session: Session) {
  const history = of(session)
  clearTimeout(history.timer)
  history.timer = undefined
  history.undo = []
  history.redo = []
  // During the setup the keyboard is still being put together; the editor starts the list.
  history.last = session.setup ? undefined : take(session)
  show(history)
}

/** A change has come to rest: what was there before it becomes a step back. */
function settle(session: Session) {
  const history = of(session)
  clearTimeout(history.timer)
  history.timer = undefined
  const now = take(session)
  if (!history.last) {
    history.last = now
    return
  }
  if (text(now) === text(history.last)) return
  history.undo.push(history.last)
  if (history.undo.length > MAX_STEPS) history.undo.shift()
  history.redo = []
  history.last = now
  show(history)
}

onChange((session, kind) => {
  const history = of(session)
  if (history.state.busy) return
  if (kind === 'reset' || session.setup) return reset(session)
  clearTimeout(history.timer)
  history.timer = setTimeout(() => settle(session), SETTLE_MS)
})

/** The numbers behind the undo and redo buttons. */
export const historyOf = (session: Session) => of(session).state

/** Call when the editor opens: steps count from what it shows first. */
export function startHistory(session: Session) {
  if (!of(session).last) reset(session)
}

/** Write a remembered state back, touching only what differs. */
async function restore(session: Session, to: Snapshot): Promise<void> {
  if (text(session.layout) !== text(to.layout)) await setLayout(session, to.layout)
  if (session.layers.length !== to.layers.length) await setLayerCount(session, to.layers.length)
  for (const [index, wanted] of to.layers.entries()) {
    const have = session.layers[index]
    // Runs of keys that differ, so one key taken back is one short message.
    for (let pos = 0; pos < wanted.length; pos++) {
      if (text(have[pos]) === text(wanted[pos])) continue
      let end = pos + 1
      while (end < wanted.length && text(have[end]) !== text(wanted[end])) end++
      const run = wanted.slice(pos, end)
      await session.client.setActions(index, pos, run)
      have.splice(pos, run.length, ...run)
      pos = end
    }
  }
  for (const [index, color] of to.layers.map((_, layer) => to.layerColors[layer] ?? 0).entries()) {
    if ((session.layerColors[index] ?? 0) !== color) await setLayerColor(session, index, color)
  }
  for (const index of to.layers.keys()) {
    if ((session.layersWith[index] ?? 0) !== (to.layersWith[index] ?? 0)) await setLayerWith(session, index, to.layersWith[index] ?? 0)
  }
  for (const [index, profile] of to.profiles.entries()) {
    if (text(session.profiles[index]) !== text(profile)) await setProfile(session, index, profile)
  }
  if (text(session.combos) !== text(to.combos)) await setCombos(session, to.combos)
  for (const [index, steps] of to.macros.entries()) {
    if (text(session.macros[index]) !== text(steps)) await setMacro(session, index, steps)
  }
  for (const [index, name] of to.macroNames.entries()) {
    if ((session.macroNames[index] ?? '') !== name) await setMacroName(session, index, name)
  }
  if (text(session.letterRows) !== text(to.letterRows)) await setLetterRows(session, to.letterRows)
  if (canLight(session) && text(session.lighting) !== text(to.lighting)) await setLighting(session, to.lighting)
  touch(session)
}

async function step(session: Session, from: 'undo' | 'redo'): Promise<boolean> {
  const history = of(session)
  if (history.state.busy) return false
  // A change made a moment ago is a step of its own.
  if (history.timer) settle(session)
  const target = history[from].pop()
  if (!target || !history.last) return false
  history[from === 'undo' ? 'redo' : 'undo'].push(history.last)
  history.state.busy = true
  try {
    await restore(session, target)
    history.last = take(session)
  } catch (error) {
    // Half written; what the keyboard holds now is the new start.
    history.state.busy = false
    reset(session)
    throw error
  } finally {
    history.state.busy = false
    show(history)
  }
  return true
}

/** Take the last change back. Resolves to false when there was none. */
export const undo = (session: Session) => step(session, 'undo')
/** Make the change that was taken back again. */
export const redo = (session: Session) => step(session, 'redo')
