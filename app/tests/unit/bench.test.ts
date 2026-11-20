// The virtual workbench: boards, their USB cables and the cable between them.
import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { app, installVirtual, openDemo, plugCable, plugUsb, reconcile, RESTART_MS } from '../../src/lib/app'
import { DEMOS } from '../../src/lib/demos'
import { save, setAction, setHardware, setLayout, starterAction, starterLayout, type Session } from '../../src/lib/session'
import { loadBridge } from '../../src/lib/wasm'

beforeAll(async () => {
  await loadBridge(readFileSync(new URL('../../src/wasm/jmk.wasm', import.meta.url)))
})

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))
const open = (index: number): Session => app.sessions.find((s) => s.id === app.benches[0].halves[index].sessionId && !s.closed)!

describe('virtual workbench', () => {
  it('starts a split as two new boards and follows power and cables', { timeout: 20_000 }, async () => {
    await openDemo(DEMOS.find((demo) => demo.id === 'split')!)
    const bench = app.benches[0]
    expect(app.sessions).toHaveLength(0)
    // No firmware: plugging in opens nothing yet.
    await plugUsb(bench, 0, true)
    await plugUsb(bench, 1, true)
    expect(app.sessions).toHaveLength(0)
    await Promise.all([installVirtual(bench, 0), installVirtual(bench, 1)])
    // Flashed, they restart before they are keyboards.
    expect(app.sessions).toHaveLength(0)
    await wait(RESTART_MS + 400)
    expect(app.sessions).toHaveLength(2)

    const matrix = { Matrix: { rows: [2, 3, 4, 5], cols: [6, 7, 8, 9, 10], diodes: 'Col2Row' as const } }
    const base = { matrix, reserved_pins: [], debounce_ms: 5, encoders: [] }
    const link = { Uart: { tx: 0, rx: 1 } }
    for (const [index, offset] of [[0, 0], [1, 20]]) {
      const half = open(index)
      await setHardware(half, { ...base, split: { Half: { link, offset } } })
      const grid = starterLayout(half.hardware)
      await setLayout(half, [...grid, ...grid])
      await setAction(half, 0, 1, starterAction(1))
      await save(half)
      half.setup = false
    }
    // The cable setting takes effect with a restart: pull both off power.
    await plugUsb(bench, 0, false)
    await plugUsb(bench, 1, false)
    await plugUsb(bench, 0, true)
    await plugUsb(bench, 1, true)
    await wait(900)
    expect((await open(1).client.splitStatus()).sync).toBe('Same')

    // The left half off USB keeps running through the cable and fetches a saved change.
    await plugUsb(bench, 0, false)
    expect(open(0)).toBeUndefined()
    const right = open(1)
    await setAction(right, 0, 1, starterAction(25))
    await save(right)
    await wait(1500)
    expect(await right.client.splitStatus()).toMatchObject({ connected: true, sync: 'Same', revision: 2, peer_revision: 2 })

    // It lost power when the cable went, and still has the change when it is back on USB.
    plugCable(bench, false)
    await plugUsb(bench, 0, true)
    const left = open(0)
    expect(left.layers[0][1]).toEqual(starterAction(25))
    expect(await reconcile(left, right)).toBeUndefined()
  })
})
