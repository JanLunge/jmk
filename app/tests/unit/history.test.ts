// Undo and redo against a virtual keyboard.
import { readFileSync } from 'node:fs'
import { beforeAll, expect, it } from 'vitest'
import { DEMOS } from '../../src/lib/demos'
import { historyOf, redo, startHistory, undo } from '../../src/lib/history'
import { openSession, save, setAction, setHardware, setLayerCount, setLayout, starterLayout, type Session } from '../../src/lib/session'
import { advanceVirtualTime, VirtualTransport } from '../../src/lib/transport'
import { loadBridge } from '../../src/lib/wasm'
import { starterAction } from './keys'

beforeAll(async () => {
  await loadBridge(readFileSync(new URL('../../src/wasm/jmk.wasm', import.meta.url)))
})

async function during<T>(work: Promise<T>): Promise<T> {
  const pump = setInterval(() => advanceVirtualTime(5), 1)
  try {
    return await work
  } finally {
    clearInterval(pump)
  }
}

/** Long enough for a change to count as finished. */
const rest = () => new Promise((done) => setTimeout(done, 450))

let uid = 200
async function pad(): Promise<Session> {
  const demo = DEMOS.find((d) => d.id === 'macropad')!
  const session = await during(openSession(new VirtualTransport(demo.name, demo.halves[0].wiring, uid++), demo.halves[0]))
  const hardware = {
    matrix: { Matrix: { rows: [16, 17], cols: [18, 19, 20], diodes: 'Col2Row' as const } },
    reserved_pins: [], debounce_ms: 5, encoders: [], split: 'None' as const,
  }
  await during(setHardware(session, hardware))
  await during(setLayout(session, starterLayout(hardware)))
  await during(setAction(session, 0, 0, starterAction(0)))
  session.setup = false
  startHistory(session)
  return session
}

it('takes changes back one step at a time, on the keyboard too, and makes them again', async () => {
  const session = await pad()
  const state = historyOf(session)
  expect(state.undo).toBe(0)

  // Two keys at once are one step; a later change is another.
  await during(setAction(session, 0, 1, starterAction(1)))
  await during(setAction(session, 0, 2, starterAction(2)))
  await rest()
  await during(setLayerCount(session, 2))
  await during(setAction(session, 1, 0, starterAction(9)))
  await rest()
  const moved = session.layout.map((key, pos) => (pos === 0 ? { ...key, x: key.x + 40 } : key))
  await during(setLayout(session, moved))
  await rest()
  expect(state.undo).toBe(3)

  // Saved in between: undo still works, the list is the app's own.
  await during(save(session))

  expect(await during(undo(session))).toBe(true)
  expect(session.layout[0].x).toBe(0)
  expect((await during(session.client.layout()))[0].x).toBe(0)

  expect(await during(undo(session))).toBe(true)
  expect(session.layers.length).toBe(1)
  expect(await during(session.client.layerCount())).toBe(1)

  expect(await during(undo(session))).toBe(true)
  expect(session.layers[0].slice(0, 3)).toEqual([starterAction(0), 'Transparent', 'Transparent'])
  expect((await during(session.client.actions(0, 3)))).toEqual(session.layers[0].slice(0, 3))
  expect(await during(undo(session))).toBe(false)
  expect(state).toMatchObject({ undo: 0, redo: 3 })

  expect(await during(redo(session))).toBe(true)
  expect((await during(session.client.actions(0, 3)))).toEqual([starterAction(0), starterAction(1), starterAction(2)])
  expect(await during(redo(session))).toBe(true)
  expect((await during(session.client.actions(1, 1)))).toEqual([starterAction(9)])

  // A new change after going back drops what could be made again.
  await during(setAction(session, 0, 1, starterAction(5)))
  await rest()
  expect(state).toMatchObject({ undo: 3, redo: 0 })
})

it('starts afresh when the wiring changes', async () => {
  const session = await pad()
  await during(setAction(session, 0, 1, starterAction(1)))
  await rest()
  expect(historyOf(session).undo).toBe(1)
  await during(setHardware(session, { ...session.hardware, debounce_ms: 9 }))
  expect(historyOf(session).undo).toBe(0)
})
