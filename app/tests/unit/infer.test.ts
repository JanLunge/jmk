import { planAddedKey } from '../../src/lib/addkey'
import { describe, expect, it } from 'vitest'
import { inferWiring, sameKey, signatureOf, type KeySignature } from '../../src/lib/infer'
import { parseKle } from '../../src/lib/layout'
import { textToSteps } from '../../src/lib/keycodes'

const link = (driven: number, sensed: number, diode = true): KeySignature => ({ kind: 'link', driven, sensed, diode })

describe('signatureOf', () => {
  it('reads one-way and two-way links', () => {
    expect(signatureOf([{ Link: { driven: 16, sensed: 18 } }])).toEqual(link(16, 18))
    expect(signatureOf([{ Link: { driven: 5, sensed: 3 } }, { Link: { driven: 3, sensed: 5 } }])).toEqual(link(3, 5, false))
    expect(signatureOf([{ Ground: 4 }])).toEqual({ kind: 'ground', pin: 4 })
  })
  it('rejects two keys at once', () => {
    expect(signatureOf([{ Ground: 4 }, { Ground: 5 }])).toBeUndefined()
  })
  it('recognises the same diodeless key in either direction', () => {
    expect(sameKey(link(3, 5, false), link(5, 3, false))).toBe(true)
    expect(sameKey(link(3, 5), link(5, 3))).toBe(false)
  })
})

describe('inferWiring', () => {
  it('finds a direct-wired pad in press order', () => {
    const result = inferWiring([{ kind: 'ground', pin: 9 }, { kind: 'ground', pin: 2 }])
    expect(result).toMatchObject({ ok: true, wiring: { matrix: { Direct: { pins: [9, 2], active_high: false } }, positions: [0, 1] } })
  })

  it('finds the real macro pad: columns driven, rows sensed', () => {
    // Pressing keys in reading order on the 3x2 pad wired to GP16..GP20.
    const keys = [link(16, 18), link(17, 18), link(16, 19), link(17, 19), link(16, 20), link(17, 20)]
    const result = inferWiring(keys)
    expect(result).toMatchObject({
      ok: true,
      wiring: { matrix: { Matrix: { rows: [16, 17], cols: [18, 19, 20], diodes: 'Col2Row' } }, positions: [0, 3, 1, 4, 2, 5] },
    })
  })

  it('names the larger group columns and flips the diode setting', () => {
    const keys = [link(2, 10), link(3, 10), link(4, 10), link(2, 11)]
    const result = inferWiring(keys)
    expect(result).toMatchObject({ ok: true, wiring: { matrix: { Matrix: { rows: [10, 11], cols: [2, 3, 4], diodes: 'Row2Col' } } } })
  })

  it('handles a matrix without diodes', () => {
    const keys = [link(2, 5, false), link(3, 6, false), link(2, 6, false)]
    const result = inferWiring(keys)
    expect(result.ok).toBe(true)
    if (result.ok && result.wiring.matrix !== 'None' && 'Matrix' in result.wiring.matrix) {
      const { rows, cols } = result.wiring.matrix.Matrix
      expect([...rows, ...cols].sort()).toEqual([2, 3, 5, 6])
      expect(new Set(result.wiring.positions).size).toBe(3)
    }
  })

  it('explains wiring that forms no rows and columns', () => {
    expect(inferWiring([link(2, 5, false), link(5, 6, false), link(2, 6, false)])).toMatchObject({ ok: false })
    expect(inferWiring([link(2, 5), link(2, 6, false)])).toMatchObject({ ok: false })
  })

  it('reads diodes pointing both ways as a duplex matrix', () => {
    // Rows 2 and 3, columns 4, 5 and 6; pressed crossing by crossing, both keys of each.
    const keys = [2, 3].flatMap((row) => [4, 5, 6].flatMap((col) => [link(row, col), link(col, row)]))
    const result = inferWiring(keys)
    if (!result.ok) throw new Error(result.problem)
    expect(result.wiring.matrix).toEqual({ Matrix: { rows: [2, 3], cols: [4, 5, 6], diodes: 'Both' } })
    // The keys found by pulling a row low take the first six positions, the others the next six.
    expect(result.wiring.positions).toEqual([0, 6, 1, 7, 2, 8, 3, 9, 4, 10, 5, 11])
  })

  it('lets one board mix a matrix with keys on pins of their own', () => {
    const result = inferWiring([
      { kind: 'ground', pin: 10 },
      link(2, 4),
      { kind: 'supply', pin: 12 },
      link(2, 5),
      { kind: 'ground', pin: 11 },
    ])
    if (!result.ok) throw new Error(result.problem)
    expect(result.wiring.matrix).toEqual({ Matrix: { rows: [2], cols: [4, 5], diodes: 'Col2Row' } })
    expect(result.wiring.more).toEqual([{ Direct: { pins: [10, 11], active_high: false } }, { Direct: { pins: [12], active_high: true } }])
    // The matrix comes first, then the keys to ground, then the one to supply.
    expect(result.wiring.positions).toEqual([2, 0, 4, 1, 3])
  })
})

describe('parseKle', () => {
  it('reads sizes, gaps and rows', () => {
    const keys = parseKle('[["Esc",{"x":1},"F1"],[{"w":1.5},"Tab","Q"]]')
    expect(keys).toEqual([
      { x: 0, y: 0, w: 100, h: 100, r: 0 },
      { x: 200, y: 0, w: 100, h: 100, r: 0 },
      { x: 0, y: 100, w: 150, h: 100, r: 0 },
      { x: 150, y: 100, w: 100, h: 100, r: 0 },
    ])
  })
  it('accepts a single row on its own', () => {
    expect(parseKle('["A","B",{"w":2},"C"]').map((k) => k.x)).toEqual([0, 100, 200])
  })
  it('accepts the site\'s raw data without outer brackets', () => {
    expect(parseKle('["A","B"],\n[{w:2},"Space"]')).toHaveLength(3)
  })
})

describe('textToSteps', () => {
  it('holds shift only while needed', () => {
    expect(textToSteps('aB#c')).toEqual([
      { Tap: 0x04 }, { Press: 0xe1 }, { Tap: 0x05 }, { Tap: 0x20 }, { Release: 0xe1 }, { Tap: 0x06 },
    ])
  })
})

import { KeyCollector, KnobFinder } from '../../src/lib/collector'

describe('KeyCollector', () => {
  it('collects distinct keys, ignores stuck contacts and repeats', () => {
    let now = 0
    const collector = new KeyCollector(() => now)
    collector.update([{ Ground: 3 }]) // there from the start: not a key
    now = 1000
    collector.update([{ Ground: 3 }, { Link: { driven: 16, sensed: 18 } }])
    collector.update([{ Ground: 3 }])
    collector.update([{ Ground: 3 }, { Link: { driven: 17, sensed: 18 } }])
    // A second key while the first is still held counts too.
    collector.update([{ Ground: 3 }, { Link: { driven: 17, sensed: 18 } }, { Link: { driven: 16, sensed: 19 } }])
    collector.update([{ Ground: 3 }])
    collector.update([{ Ground: 3 }, { Link: { driven: 16, sensed: 18 } }])
    expect(collector.keys).toHaveLength(3)
    expect(collector.last).toBe(0)
    expect(collector.stuck).toEqual([{ Ground: 3 }])
    expect([...collector.active].sort()).toEqual([16, 18])
  })

  it('flags two keys pressed in the same instant', () => {
    let now = 1000
    const collector = new KeyCollector(() => now - 1000 + 600)
    now = 2000
    collector.update([{ Ground: 4 }, { Ground: 5 }])
    expect(collector.confused).toBe(true)
    expect(collector.keys).toHaveLength(0)
  })
})

describe('KnobFinder', () => {
  it('finds the two pins that keep touching ground', () => {
    const finder = new KnobFinder(new Set([7]))
    const frames = [[10], [10, 11], [11], [], [10], [7, 11]]
    const results = frames.map((pins) => finder.update(pins.map((pin) => ({ Ground: pin }))))
    expect(results.at(-1)).toEqual([10, 11])
    expect(results.slice(0, 4).every((r) => r === undefined)).toBe(true)
  })
})

describe('adding one key to existing wiring', () => {
  const base = { reserved_pins: [], debounce_ms: 5, encoders: [], split: 'None' as const }
  const matrix = { ...base, matrix: { Matrix: { rows: [1, 2], cols: [10, 11], diodes: 'Row2Col' as const } } }

  it('finds an unused crossing without changing the wiring', () => {
    // Row2Col: the columns are switched, the rows are read.
    const plan = planAddedKey(matrix, { kind: 'link', driven: 11, sensed: 2, diode: true })
    expect(plan).toMatchObject({ ok: true, rewired: false, pos: 3 })
  })

  it('adds a row and renumbers nothing, adds a column and renumbers the rows below', () => {
    const row = planAddedKey(matrix, { kind: 'link', driven: 10, sensed: 3, diode: true })
    if (!row.ok) throw new Error(row.problem)
    expect(row.hardware.matrix).toEqual({ Matrix: { rows: [1, 2, 3], cols: [10, 11], diodes: 'Row2Col' } })
    expect([row.pos, ...[0, 1, 2, 3].map(row.move)]).toEqual([4, 0, 1, 2, 3])

    const col = planAddedKey(matrix, { kind: 'link', driven: 12, sensed: 2, diode: true })
    if (!col.ok) throw new Error(col.problem)
    expect(col.hardware.matrix).toEqual({ Matrix: { rows: [1, 2], cols: [10, 11, 12], diodes: 'Row2Col' } })
    // Positions after the switches (knobs, the other half) shift by the two new crossings.
    expect([col.pos, ...[0, 1, 2, 3, 4].map(col.move)]).toEqual([5, 0, 1, 3, 4, 6])
  })

  it('accepts a diodeless key either way round and refuses a backwards diode', () => {
    expect(planAddedKey(matrix, { kind: 'link', driven: 2, sensed: 11, diode: false })).toMatchObject({ ok: true, pos: 3 })
    expect(planAddedKey(matrix, { kind: 'link', driven: 2, sensed: 11, diode: true }).ok).toBe(false)
    expect(planAddedKey(matrix, { kind: 'link', driven: 1, sensed: 2, diode: false }).ok).toBe(false)
    // A key on a pin of its own joins as a group after the matrix.
    expect(planAddedKey(matrix, { kind: 'ground', pin: 5 })).toMatchObject({ ok: true, hardware: { more: [{ Direct: { pins: [5], active_high: false } }] } })
  })

  it('appends a pin to one-pin-per-key wiring', () => {
    const direct = { ...base, matrix: { Direct: { pins: [4, 5], active_high: false } } }
    const plan = planAddedKey(direct, { kind: 'ground', pin: 7 })
    if (!plan.ok) throw new Error(plan.problem)
    expect(plan.hardware.matrix).toEqual({ Direct: { pins: [4, 5, 7], active_high: false } })
    expect([plan.pos, plan.move(1), plan.move(2)]).toEqual([2, 1, 3])
    expect(planAddedKey(direct, { kind: 'ground', pin: 5 })).toMatchObject({ ok: true, rewired: false, pos: 1 })
    expect(planAddedKey(direct, { kind: 'supply', pin: 7 })).toMatchObject({ ok: true, pos: 2, hardware: { more: [{ Direct: { pins: [7], active_high: true } }] } })
  })
})

describe('a missed key on a board that mixes wiring styles', () => {
  const hardware = {
    matrix: { Matrix: { rows: [2, 3], cols: [4, 5], diodes: 'Both' as const } },
    more: [{ Direct: { pins: [10], active_high: false } }],
    reserved_pins: [], debounce_ms: 5, encoders: [], split: 'None' as const,
  }

  it('finds the second key of a crossing and a key on a pin of its own', () => {
    const second = planAddedKey(hardware, link(5, 3))
    if (!second.ok) throw new Error(second.problem)
    // Row 3, column 5 is crossing 3; its second key comes after the four first ones.
    expect([second.pos, second.rewired]).toEqual([7, false])

    const direct = planAddedKey(hardware, { kind: 'ground', pin: 11 })
    if (!direct.ok) throw new Error(direct.problem)
    expect(direct.hardware.more).toEqual([{ Direct: { pins: [10, 11], active_high: false } }])
    expect([direct.pos, direct.move(8), direct.move(9)]).toEqual([9, 8, 10])

    const supply = planAddedKey(hardware, { kind: 'supply', pin: 12 })
    if (!supply.ok) throw new Error(supply.problem)
    expect(supply.hardware.more).toEqual([{ Direct: { pins: [10], active_high: false } }, { Direct: { pins: [12], active_high: true } }])
    expect(supply.pos).toBe(9)
  })

  it('renumbers both halves of a duplex matrix when it grows a column', () => {
    const plan = planAddedKey(hardware, link(6, 2))
    if (!plan.ok) throw new Error(plan.problem)
    expect(plan.hardware.matrix).toEqual({ Matrix: { rows: [2, 3], cols: [4, 5, 6], diodes: 'Both' } })
    // Second key of row 2, column 6.
    expect(plan.pos).toBe(8)
    expect([0, 1, 2, 3, 4, 5, 6, 7, 8].map(plan.move)).toEqual([0, 1, 3, 4, 6, 7, 9, 10, 12])
  })
})
