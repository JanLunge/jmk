// The app's session layer against virtual keyboards, without any UI.
import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { KnobFinder } from '../../src/lib/collector'
import { DEMOS } from '../../src/lib/demos'
import type { Contact, HostEvent } from '../../src/lib/protocol'
import { adoptRemoteKeys, openSession, setAction, setHardware, setLayerCount, starterAction, starterLayout, setLayout, type Session } from '../../src/lib/session'
import { advanceVirtualTime, VirtualTransport } from '../../src/lib/transport'
import { loadBridge } from '../../src/lib/wasm'

beforeAll(async () => {
  await loadBridge(readFileSync(new URL('../../src/wasm/jmk.wasm', import.meta.url)))
})

let uid = 100
async function open(demoId: string): Promise<{ session: Session; transport: VirtualTransport; typed: HostEvent[] }> {
  const demo = DEMOS.find((d) => d.id === demoId)!
  const transport = new VirtualTransport(demo.name, demo.halves[0].wiring, uid++)
  const pending = openSession(transport, demo.halves[0])
  // Requests are answered when virtual time moves.
  const pump = setInterval(() => advanceVirtualTime(5), 1)
  const session = await pending
  clearInterval(pump)
  return { session, transport, typed: session.typed }
}

/** Run an async action while virtual time keeps moving. */
async function during<T>(work: Promise<T>): Promise<T> {
  const pump = setInterval(() => advanceVirtualTime(5), 1)
  try {
    return await work
  } finally {
    clearInterval(pump)
  }
}

const keys = (typed: HostEvent[]) => typed.flatMap((e) => ('Keyboard' in e ? [e.Keyboard.keys] : []))

describe('virtual macro pad', () => {
  it('types through layers set from the app', async () => {
    const { session, transport, typed } = await open('macropad')
    const hardware = {
      matrix: { Matrix: { rows: [16, 17], cols: [18, 19, 20], diodes: 'Col2Row' as const } },
      reserved_pins: [], debounce_ms: 5, encoders: [], split: 'None' as const,
    }
    await during(setHardware(session, hardware))
    await during(setLayout(session, starterLayout(hardware)))
    await during(setAction(session, 0, 1, { Basic: { Key: 0x05 } }))
    await during(setLayerCount(session, 2))
    await during(setAction(session, 1, 1, { Basic: { Key: 0x1e } }))
    await during(setAction(session, 0, 0, { Basic: { MomentaryLayer: 1 } }))

    transport.setKey(0, true)
    advanceVirtualTime(20)
    transport.setKey(1, true)
    advanceVirtualTime(20)
    expect(keys(typed).at(-1)).toEqual([0x1e])
  })
})

describe('virtual knob pad', () => {
  it('lets the app find the knob pins while probing', async () => {
    const { session, transport } = await open('knobpad')
    const finder = new KnobFinder(new Set())
    let found: [number, number] | undefined
    const seen: Contact[][] = []
    session.client.onProbe = (contacts) => {
      seen.push(contacts)
      found ??= finder.update(contacts)
    }
    await during(session.client.startProbe([...Array(23).keys()]))
    advanceVirtualTime(100)
    for (let i = 0; i < 4; i++) {
      transport.turnEncoder(0, true)
      advanceVirtualTime(140)
    }
    expect(seen.length).toBeGreaterThan(0)
    expect(found?.slice().sort()).toEqual([10, 11])
  })
})

describe('virtual split keyboard', () => {
  it('adopts the other half and types its keys', async () => {
    const demo = DEMOS.find((d) => d.id === 'split')!
    const transports = demo.halves.map((half, i) => new VirtualTransport(half.label, half.wiring, 200 + i))
    VirtualTransport.connect(transports[0], transports[1])
    const [left, right] = [await during(openSession(transports[0])), await during(openSession(transports[1]))]
    const matrix = { Matrix: { rows: [2, 3, 4, 5], cols: [6, 7, 8, 9, 10], diodes: 'Col2Row' as const } }
    const base = { matrix, reserved_pins: [], debounce_ms: 5, encoders: [] }
    const link = { Uart: { tx: 0, rx: 1 } }
    await during(setHardware(left, { ...base, split: { Central: link } }))
    await during(setLayout(left, starterLayout(left.hardware)))
    await during(setAction(left, 0, 0, starterAction(0)))
    await during(setHardware(right, { ...base, split: { Peripheral: link } }))
    await during(setLayout(right, starterLayout(right.hardware)))

    advanceVirtualTime(200)
    left.split = await during(left.client.splitStatus())
    expect(left.split).toEqual({ connected: true, remote_positions: 20 })
    expect(await during(adoptRemoteKeys(left, right))).toBe(true)
    expect(left.layout).toHaveLength(40)

    transports[1].setKey(0, true)
    advanceVirtualTime(30)
    const lastKeys = left.typed.flatMap((e) => ('Keyboard' in e ? [e.Keyboard.keys] : [])).at(-1)
    expect(lastKeys).toEqual([0x05]) // "b": the second letter handed out
    expect([...left.pressed]).toEqual([20])
  })
})
