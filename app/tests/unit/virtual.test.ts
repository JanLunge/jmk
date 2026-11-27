// The app's session layer against virtual keyboards, without any UI.
import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { KnobFinder } from '../../src/lib/collector'
import { planAddedKey } from '../../src/lib/addkey'
import { addSession, app, reconcile } from '../../src/lib/app'
import { chooseLink, findWires } from '../../src/lib/cable'
import { signatureOf } from '../../src/lib/infer'
import { DEMOS } from '../../src/lib/demos'
import type { Contact, HostEvent } from '../../src/lib/protocol'
import { lightMode, placeAtKey, hsvToRgb, rgbToHsv, ring, underKeys } from '../../src/lib/lighting'
import { keymapPoster } from '../../src/lib/poster'
import { layerHex } from '../../src/lib/layers'
import { gridLayout } from '../../src/lib/layout'
import { addKey, alignHalves, knobPositions, needsRestart, save, setLighting, openSession, setAction, setHardware, setLayerCount, starterLayout, setLayout, type Session } from '../../src/lib/session'
import { starterAction } from './keys'
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
    // The keyboard says which layers are on and which one each press used.
    expect(session.layerAware).toBe(true)
    expect(session.activeLayers).toBe(0b11)
    expect([...session.usedLayer]).toEqual([[0, 0], [1, 1]])

    transport.setKey(1, false)
    transport.setKey(0, false)
    advanceVirtualTime(20)
    expect(session.activeLayers).toBe(0b01)
    expect(session.usedLayer.size).toBe(0)
  })
})

describe('a key that was missed during setup', () => {
  it('extends the matrix and keeps every other key where it was', async () => {
    const { session, transport, typed } = await open('macropad')
    // Set up without the third column, as if its two keys had been skipped.
    const hardware = {
      matrix: { Matrix: { rows: [16, 17], cols: [18, 19], diodes: 'Col2Row' as const } },
      reserved_pins: [], debounce_ms: 5, encoders: [], split: 'None' as const,
    }
    await during(setHardware(session, hardware))
    await during(setLayout(session, starterLayout(hardware)))
    for (const pos of [0, 1, 2, 3]) await during(setAction(session, 0, pos, starterAction(pos)))

    let seen: Contact[] = []
    session.client.onProbe = (contacts) => (seen = contacts)
    await during(session.client.startProbe([16, 17, 18, 19, 20]))
    advanceVirtualTime(100)
    transport.setKey(2, true) // first row, third column
    advanceVirtualTime(100)
    const key = signatureOf(seen)!
    transport.setKey(2, false)
    await during(session.client.stopProbe())

    const plan = planAddedKey(session.hardware, key)
    if (!plan.ok) throw new Error(plan.problem)
    expect(plan.rewired).toBe(true)
    expect(await during(addKey(session, plan))).toBe(2)
    expect(session.hardware.matrix).toEqual({ Matrix: { rows: [16, 17], cols: [18, 19, 20], diodes: 'Col2Row' } })
    // The second row moved from positions 2, 3 to 3, 4 and kept its letters.
    expect(session.layers[0].slice(0, 5)).toEqual([starterAction(0), starterAction(1), 'Transparent', starterAction(2), starterAction(3)])
    expect(session.layout.filter((k) => k.w > 0).length).toBe(5)

    const tapKey = (index: number) => {
      transport.setKey(index, true)
      advanceVirtualTime(20)
      transport.setKey(index, false)
      advanceVirtualTime(20)
    }
    // The added key starts empty, like every key of a new keyboard.
    await during(setAction(session, 0, 2, starterAction(4)))
    tapKey(3) // second row, first column: still C
    tapKey(2) // the added key: E
    expect(keys(typed).filter((k) => k.length)).toEqual([[0x06], [0x08]])

    // The same key again is recognised as present.
    const again = planAddedKey(session.hardware, key)
    expect(again.ok && !again.rewired && again.pos === 2).toBe(true)
  })
})

describe('the cable between two halves', () => {
  /** Two fresh halves joined by a cable with these data wires. */
  async function halves(wires: [number, number][]) {
    const demo = DEMOS.find((d) => d.id === 'split')!
    const transports = demo.halves.map((half) => new VirtualTransport(half.label, half.wiring, uid++))
    VirtualTransport.connect(transports[0], transports[1], wires)
    const left = await during(openSession(transports[0], demo.halves[0]))
    const right = await during(openSession(transports[1], demo.halves[1]))
    return { left, right }
  }
  const pins = [...Array(16).keys()]
  /** Stands in for waiting: lets the virtual keyboards run that long. */
  const wait = async (ms: number) => {
    advanceVirtualTime(ms)
    await new Promise((resolve) => setTimeout(resolve, 0))
  }

  it('is found wire by wire, whichever way it is crossed', async () => {
    const { left, right } = await halves([[0, 1], [1, 0]])
    const wires = await during(findWires(left, right, pins, pins, wait))
    expect(wires).toEqual([{ a: 0, b: 1 }, { a: 1, b: 0 }])
    expect(chooseLink(wires, 'Rp2040', 'Rp2040')).toEqual({ ok: true, a: { tx: 0, rx: 1 }, b: { tx: 0, rx: 1 } })

    // The halves type again afterwards.
    await during(setHardware(left, { matrix: { Direct: { pins: [20], active_high: false } }, reserved_pins: [], debounce_ms: 5, encoders: [], split: 'None' }))
    expect(left.hardware.matrix).not.toBe('None')

    // A straight cable to other pins: the directions are assigned to fit the chip.
    const straight = await halves([[12, 13], [13, 12]])
    const found = await during(findWires(straight.left, straight.right, pins, pins, wait))
    expect(chooseLink(found, 'Rp2040', 'Rp2040')).toEqual({ ok: true, a: { tx: 12, rx: 13 }, b: { tx: 12, rx: 13 } })
  })

  it('says plainly what is wrong with a cable it cannot use', async () => {
    const none = await halves([])
    expect(await during(findWires(none.left, none.right, pins, pins, wait))).toEqual([])
    expect(chooseLink([], 'Rp2040', 'Rp2040')).toMatchObject({ ok: false, problem: expect.stringContaining('No wire') })
    expect(chooseLink([{ a: 0, b: 1 }], 'Rp2040', 'Rp2040')).toMatchObject({ ok: false, problem: expect.stringContaining('update the firmware') })
    expect(chooseLink([{ a: 0, b: 1 }], 'Nrf52840', 'Nrf52840')).toMatchObject({ ok: false, problem: expect.stringContaining('needs two') })
    // Firmware that can take turns on a wire needs only the one, on any pin.
    expect(chooseLink([{ a: 3, b: 7 }], 'Rp2040', 'Rp2040', undefined, true)).toEqual({ ok: true, a: { tx: 3, rx: 3 }, b: { tx: 7, rx: 7 }, shared: true })
    expect(chooseLink([{ a: 0, b: 1 }, { a: 12, b: 0 }], 'Rp2040', 'Rp2040', undefined, true)).toMatchObject({ ok: true, shared: true, a: { tx: 0, rx: 0 } })
    // Two wires that suit a serial port are still used as two.
    expect(chooseLink([{ a: 0, b: 1 }, { a: 1, b: 0 }], 'Rp2040', 'Rp2040', undefined, true)).toEqual({ ok: true, a: { tx: 0, rx: 1 }, b: { tx: 0, rx: 1 } })
    // Both wires on sending pins of this half: no serial port fits.
    expect(chooseLink([{ a: 0, b: 1 }, { a: 12, b: 0 }], 'Rp2040', 'Rp2040')).toMatchObject({ ok: false, problem: expect.stringContaining('cannot run a serial connection') })
    // The other chip takes any pins.
    expect(chooseLink([{ a: 2, b: 7 }, { a: 3, b: 9 }], 'Nrf52840', 'Nrf52840')).toEqual({ ok: true, a: { tx: 2, rx: 3 }, b: { tx: 9, rx: 7 } })
    // An RP2350 has a serial port on more pins than an RP2040, and takes turns on one wire as well.
    expect(chooseLink([{ a: 2, b: 3 }, { a: 3, b: 2 }], 'Rp2350', 'Rp2350')).toEqual({ ok: true, a: { tx: 2, rx: 3 }, b: { tx: 2, rx: 3 } })
    expect(chooseLink([{ a: 2, b: 3 }, { a: 3, b: 2 }], 'Rp2040', 'Rp2040')).toMatchObject({ ok: false })
    expect(chooseLink([{ a: 2, b: 5 }, { a: 3, b: 4 }], 'Rp2350', 'Rp2040')).toEqual({ ok: true, a: { tx: 2, rx: 3 }, b: { tx: 4, rx: 5 } })
    expect(chooseLink([{ a: 2, b: 4 }, { a: 3, b: 6 }], 'Rp2350', 'Rp2350')).toMatchObject({ ok: false, problem: expect.stringContaining('An RP2350 also') })
    expect(chooseLink([{ a: 0, b: 1 }], 'Rp2350', 'Rp2350')).toMatchObject({ ok: false, problem: expect.stringContaining('update the firmware') })
    // Spare wires are skipped until a pair fits.
    expect(chooseLink([{ a: 2, b: 3 }, { a: 4, b: 9 }, { a: 5, b: 8 }], 'Rp2040', 'Rp2040')).toEqual({ ok: true, a: { tx: 4, rx: 5 }, b: { tx: 8, rx: 9 } })
  })
})

describe('a keyboard that comes back', () => {
  it('takes its old place instead of opening a second time', async () => {
    const first = await open('macropad')
    const other = await open('macropad')
    addSession(first.session)
    addSession(other.session)
    expect(app.sessions.length).toBe(2)
    app.currentId = first.session.id

    // Unplugged, then plugged back in: same serial number, new connection.
    first.session.closed = true
    const demo = DEMOS.find((d) => d.id === 'macropad')!
    const transport = new VirtualTransport(demo.name, demo.halves[0].wiring, first.session.info.uid[0])
    const back = await during(openSession(transport, demo.halves[0]))
    addSession(back)

    expect(app.sessions.length).toBe(2)
    expect(app.sessions[0].closed).toBe(false)
    expect(app.sessions[0].id).toBe(first.session.id)
    expect(app.currentId).toBe(first.session.id)
    expect(app.sessions[1].id).toBe(other.session.id)
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

describe('lights', () => {
  it('shine on a virtual keyboard as soon as it is told the pin', async () => {
    const { session, transport } = await open('knobpad')
    const hardware = {
      matrix: { Direct: { pins: [2, 3, 4, 5], active_high: false } },
      reserved_pins: [], debounce_ms: 5, encoders: [{ a: 10, b: 11, steps_per_detent: 4 }], split: 'None' as const,
    }
    await during(setHardware(session, hardware))
    await during(setLayout(session, starterLayout(hardware)))
    // The knob has a place in the picture, at the first of its two positions.
    expect(knobPositions(session)).toEqual([4])
    expect(session.layout[4].w).toBeGreaterThan(0)
    expect(session.layout[5].w).toBe(0)

    const red = { effect: 'Solid' as const, hue: 0, sat: 255, val: 255, speed: 0 }
    const leds = ring(session.layout, 3)
    leds[0] = { key: 1, ...placeAtKey(session.layout, 1) }
    await during(setLighting(session, { pin: 16, order: 'Grb', limit: 255, leds, keys: red, underglow: { ...red, effect: 'Off' } }))
    await during(save(session))
    expect(needsRestart(session)).toBe(false)
    expect(session.lighting.leds[0].key).toBe(1)
    advanceVirtualTime(40)
    // As sent down the chain: green first.
    expect(session.lights).toEqual([[0, 255, 0], [0, 0, 0], [0, 0, 0]])

    // A pin in use for keys is refused.
    await expect(during(session.client.setLighting({ ...session.lighting, pin: 3 }))).rejects.toThrow('used twice')
    transport.close()
  })

  it('converts colours the way the firmware does', () => {
    expect(hsvToRgb(0, 255, 255)).toEqual([255, 0, 0])
    expect(hsvToRgb(85, 255, 255)[1]).toBe(255)
    expect(hsvToRgb(10, 0, 77)).toEqual([77, 77, 77])
    const { hue, sat } = rgbToHsv(0, 0, 255)
    const blue = hsvToRgb(hue, sat, 255)
    expect(blue[2]).toBe(255)
    expect(blue[0] + blue[1]).toBeLessThan(20)
  })
})

describe('virtual split keyboard', () => {
  it('lets either half type for both and keeps their keymaps the same', async () => {
    const demo = DEMOS.find((d) => d.id === 'split')!
    const transports = demo.halves.map((half, i) => new VirtualTransport(half.label, half.wiring, 200 + i))
    VirtualTransport.connect(transports[0], transports[1])
    const [left, right] = [await during(openSession(transports[0])), await during(openSession(transports[1]))]
    const matrix = { Matrix: { rows: [2, 3, 4, 5], cols: [6, 7, 8, 9, 10], diodes: 'Col2Row' as const } }
    const base = { matrix, reserved_pins: [], debounce_ms: 5, encoders: [] }
    const link = { Uart: { tx: 0, rx: 1 } }
    await during(setHardware(left, { ...base, split: { Half: { link, offset: 0 } } }))
    await during(setHardware(right, { ...base, split: { Half: { link, offset: 20 } } }))
    const half = starterLayout(left.hardware)
    await during(setLayout(left, [...half, ...half]))
    await during(setAction(left, 0, 0, starterAction(0)))
    await during(setAction(left, 0, 20, starterAction(1)))

    advanceVirtualTime(1200)
    left.split = await during(left.client.splitStatus())
    right.split = await during(right.client.splitStatus())
    expect(left.split).toMatchObject({ connected: true, remote_positions: 20, leading: true, sync: 'Unknown' })
    expect(right.split).toMatchObject({ connected: true, leading: false, peer_uid: left.info.uid })

    // The second half is given what the first holds.
    expect(await during(alignHalves(left, right))).toBe(true)
    expect(right.layout).toHaveLength(40)
    expect(right.layers[0][20]).toEqual(starterAction(1))
    advanceVirtualTime(1200)
    expect((await during(left.client.splitStatus())).sync).toBe('Same')
    expect(await during(alignHalves(left, right))).toBe(false)

    const lastKeys = (session: typeof left) => session.typed.flatMap((e) => ('Keyboard' in e ? [e.Keyboard.keys] : [])).at(-1)
    transports[1].setKey(0, true)
    advanceVirtualTime(30)
    expect(lastKeys(left)).toEqual([0x05]) // "b": the first key of the second half
    expect([...left.pressed]).toEqual([20])
    expect([...right.pressed]).toEqual([20])
    expect(right.typed).toHaveLength(0)
    transports[1].setKey(0, false)
    advanceVirtualTime(30)

    // With only the second half plugged into the computer, it types for both.
    transports[0].setUsb(false)
    advanceVirtualTime(200)
    transports[0].setKey(0, true)
    advanceVirtualTime(30)
    expect(lastKeys(right)).toEqual([0x04])

    // A change made on one half reaches the other while both are open.
    left.client.mirror = right.client
    await during(setAction(left, 0, 1, starterAction(5)))
    expect((await during(right.client.actions(0, 2)))[1]).toEqual(starterAction(5))
    left.client.mirror = undefined

    // Saved on one half, a change travels to the other through their cable.
    transports[0].setUsb(true)
    await during(save(left))
    await during(save(right))
    advanceVirtualTime(1500)
    await during(setAction(left, 0, 2, starterAction(7)))
    await during(save(left))
    advanceVirtualTime(2000)
    expect((await during(right.client.actions(0, 3)))[2]).toEqual(starterAction(7))
    const [onLeft, onRight] = [await during(left.client.splitStatus()), await during(right.client.splitStatus())]
    expect(onLeft.sync).toBe('Same')
    expect([onRight.revision, onRight.digest]).toEqual([onLeft.revision, onLeft.digest])

    // Without the cable, both on USB, the app brings the older half up to date.
    left.setup = right.setup = false
    transports[0].setCable(false)
    advanceVirtualTime(500)
    await during(setAction(right, 0, 3, starterAction(9)))
    await during(save(right))
    expect(await during(reconcile(left, right))).toBe(left)
    expect(left.layers[0][3]).toEqual(starterAction(9))
    expect(await during(reconcile(left, right))).toBeUndefined()

    // Both changed while apart: that is the user's call.
    await during(setAction(left, 0, 4, starterAction(1)))
    await during(save(left))
    await during(setAction(right, 0, 4, starterAction(2)))
    await during(save(right))
    expect(await during(reconcile(left, right))).toBe('conflict')
    // Joined again they wait too, until one is chosen.
    transports[0].setCable(true)
    advanceVirtualTime(1500)
    expect((await during(left.client.splitStatus())).sync).toBe('Conflict')
    await during(left.client.resolveSplit(true))
    advanceVirtualTime(2000)
    expect((await during(right.client.actions(0, 5)))[4]).toEqual(starterAction(1))
    expect((await during(right.client.splitStatus())).sync).toBe('Same')
  })
})

describe('lights and pictures', () => {
  const layout = gridLayout({ Matrix: { rows: [1, 2], cols: [3, 4, 5], diodes: 'Col2Row' } })

  it('underglow starts as a ring around the keyboard, per key as one light per key', () => {
    const around = ring(layout, 10)
    expect(around).toHaveLength(10)
    expect(lightMode(around)).toBe('underglow')
    // Every light sits on the edge: 3 keys wide, 2 high.
    expect(around.every((led) => led.x === 0 || led.x === 255 || led.y === 0 || led.y === 170)).toBe(true)
    expect(new Set(around.map((led) => `${led.x},${led.y}`)).size).toBe(10)

    const under = underKeys(layout, 6, [5])
    expect(lightMode(under)).toBe('keys')
    expect(under.map((led) => led.key)).toEqual([0, 1, 2, 3, 4, 0])
  })

  it('layers have a colour and the whole keymap draws as one picture', () => {
    expect(layerHex([], 0)).toBeUndefined()
    expect(layerHex([], 1)).toBe('#f59e0b')
    expect(layerHex([0, 4], 1)).toBe('#ef4444')
    const source = {
      name: 'Test <pad>',
      layout,
      layers: [layout.map((_, i) => ({ Basic: i === 0 ? { MomentaryLayer: 1 } : { Key: 0x04 + i } })), layout.map(() => 'Transparent' as const)],
      layerColors: [0, 4],
      knobs: [],
      combos: [{ keys: [1, 2], action: { Basic: { Key: 0x29 } }, timeout_ms: 50 }],
    }
    for (const mode of ['grid', 'stacked'] as const) {
      const poster = keymapPoster(source, { mode, dark: false })
      expect(poster.svg).toContain('Test &lt;pad&gt;')
      expect(poster.svg).toContain('Layer 1')
      expect(poster.svg).toContain('#ef4444')
      expect(poster.svg).toContain('B + C')
      expect(poster.width).toBeGreaterThan(100)
    }
  })
})
