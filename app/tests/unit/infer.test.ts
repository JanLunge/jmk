import { planAddedKey } from '../../src/lib/addkey'
import { describe, expect, it } from 'vitest'
import { inferWiring, sameKey, signatureOf, type KeySignature } from '../../src/lib/infer'
import type { Contact } from '../../src/lib/protocol'
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

describe('a board that mixes keys with and without a diode', () => {
  const matrix = [link(2, 4), link(2, 5), link(3, 4), link(3, 5)]

  it('reads a key without a diode on two pins of its own as a group of its own', () => {
    // The push switch of a knob, between two pins no other key uses.
    const result = inferWiring([...matrix, link(8, 9, false)])
    if (!result.ok) throw new Error(result.problem)
    expect(result.wiring.matrix).toEqual({ Matrix: { rows: [2, 3], cols: [4, 5], diodes: 'Col2Row' } })
    expect(result.wiring.more).toEqual([{ Matrix: { rows: [8], cols: [9], diodes: 'Col2Row' } }])
    expect(result.wiring.positions).toEqual([0, 1, 2, 3, 4])
    expect(result.wiring.doubles).toEqual([])
    expect(result.wiring.notes).toContain('One key joins two pins of its own, without a diode.')
  })

  it('puts the larger matrix first whichever was pressed first', () => {
    const result = inferWiring([link(8, 9, false), ...matrix, { kind: 'ground', pin: 12 }])
    if (!result.ok) throw new Error(result.problem)
    expect(result.wiring.more).toEqual([{ Matrix: { rows: [8], cols: [9], diodes: 'Col2Row' } }, { Direct: { pins: [12], active_high: false } }])
    expect(result.wiring.positions).toEqual([4, 0, 1, 2, 3, 5])
  })

  it('places a key without a diode among keys with one, whichever way round it was seen', () => {
    // Row 3, column 6; the probe names the lower pin first.
    const result = inferWiring([...matrix, link(3, 6, false), link(2, 6)])
    if (!result.ok) throw new Error(result.problem)
    expect(result.wiring.matrix).toEqual({ Matrix: { rows: [2, 3], cols: [4, 5, 6], diodes: 'Col2Row' } })
    expect(result.wiring.positions).toEqual([0, 1, 3, 4, 5, 2])
    expect(result.wiring.doubles).toEqual([])
    // Pressed first, it must not decide which pins are rows.
    const early = inferWiring([link(5, 12, false), link(10, 4), link(10, 5), link(11, 4), link(11, 5), link(12, 4)])
    if (!early.ok) throw new Error(early.problem)
    expect(early.wiring.matrix).toEqual({ Matrix: { rows: [4, 5], cols: [10, 11, 12], diodes: 'Row2Col' } })
    expect(early.wiring.doubles).toEqual([])
  })

  it('reads a key without a diode in a duplex matrix as one key', () => {
    // Rows 2 and 3, columns 4 and 5, two keys per crossing; the last crossing has one key without a diode.
    const keys = [link(2, 4), link(4, 2), link(2, 5), link(5, 2), link(3, 4), link(4, 3), link(3, 5, false)]
    const result = inferWiring(keys)
    if (!result.ok) throw new Error(result.problem)
    expect(result.wiring.matrix).toEqual({ Matrix: { rows: [2, 3], cols: [4, 5], diodes: 'Both' } })
    expect(result.wiring.positions).toEqual([0, 4, 1, 5, 2, 6, 3])
    // Its crossing's second position closes with it and is not reported.
    expect(result.wiring.doubles).toEqual([7])
    // A key with a diode on the same two pins cannot be told from it.
    expect(inferWiring([...keys, link(5, 3)])).toMatchObject({ ok: false })
  })

  it('counts the positions read twice from where their group starts', () => {
    const result = inferWiring([link(8, 9, false), link(2, 4), link(4, 2), link(2, 5, false), { kind: 'ground', pin: 12 }])
    if (!result.ok) throw new Error(result.problem)
    expect(result.wiring.matrix).toEqual({ Matrix: { rows: [2], cols: [4, 5], diodes: 'Both' } })
    expect(result.wiring.positions).toEqual([4, 0, 2, 1, 5])
    expect(result.wiring.doubles).toEqual([3])
  })

  it('shares a matrix among the smallest groups when the keyboard cannot read so many apart', () => {
    const lone = [link(8, 9, false), link(10, 11, false), link(12, 13, false)]
    const keys = [...matrix, ...lone, { kind: 'ground', pin: 20 } as const, { kind: 'supply', pin: 21 } as const]
    const apart = inferWiring(keys)
    if (!apart.ok) throw new Error(apart.problem)
    expect(apart.wiring.more).toHaveLength(5)
    // Four groups, as older firmware has: the matrix, one for the lone keys, ground and supply.
    const result = inferWiring(keys, 4)
    if (!result.ok) throw new Error(result.problem)
    expect(result.wiring.more).toEqual([
      { Matrix: { rows: [8, 10, 12], cols: [9, 11, 13], diodes: 'Col2Row' } },
      { Direct: { pins: [20], active_high: false } },
      { Direct: { pins: [21], active_high: true } },
    ])
    expect(result.wiring.positions).toEqual([0, 1, 2, 3, 4, 8, 12, 13, 14])
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

  it('reads a key without a diode as one key when one way round shows first', () => {
    let now = 1000
    const collector = new KeyCollector(() => now)
    now = 2000
    const [there, back]: Contact[] = [{ Link: { driven: 5, sensed: 3 } }, { Link: { driven: 3, sensed: 5 } }]
    collector.update([there])
    now += 40
    collector.update([there, back])
    expect(collector.keys).toEqual([link(3, 5, false)])
    // Pressed again later and seen one way only, it is still that key.
    collector.update([])
    now += 1000
    collector.update([there])
    expect([collector.keys.length, collector.last]).toEqual([1, 0])

    // The two keys of a duplex crossing, the second pressed while the first is held, stay two.
    const duplex = new KeyCollector(() => now)
    now += 1000
    duplex.update([there])
    now += 400
    duplex.update([there, back])
    expect(duplex.keys).toEqual([link(5, 3), link(3, 5)])
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

describe('a missed key on a board with more than one matrix', () => {
  const hardware = {
    matrix: { Matrix: { rows: [2, 3], cols: [4, 5], diodes: 'Col2Row' as const } },
    more: [{ Matrix: { rows: [8], cols: [9], diodes: 'Col2Row' as const } }],
    reserved_pins: [], debounce_ms: 5, encoders: [], split: 'None' as const,
  }

  it('grows the matrix the key shares a pin with', () => {
    const plan = planAddedKey(hardware, link(8, 10, false))
    if (!plan.ok) throw new Error(plan.problem)
    expect(plan.hardware.matrix).toEqual(hardware.matrix)
    expect(plan.hardware.more).toEqual([{ Matrix: { rows: [8], cols: [9, 10], diodes: 'Col2Row' } }])
    expect(plan.pos).toBe(5)
    expect(planAddedKey(hardware, link(9, 8, false))).toMatchObject({ ok: true, rewired: false, pos: 4 })
  })

  it('gives a key on two unused pins a group of its own while there is room', () => {
    const plan = planAddedKey(hardware, link(10, 11, false))
    if (!plan.ok) throw new Error(plan.problem)
    expect(plan.hardware.more).toEqual([...hardware.more, { Matrix: { rows: [10], cols: [11], diodes: 'Col2Row' } }])
    expect([plan.pos, plan.move(4), plan.move(5)]).toEqual([5, 4, 6])
    // Without room it joins the first matrix as a new row and column.
    const full = planAddedKey(hardware, link(10, 11, false), 1)
    if (!full.ok) throw new Error(full.problem)
    expect(full.hardware.matrix).toEqual({ Matrix: { rows: [2, 3, 10], cols: [4, 5, 11], diodes: 'Col2Row' } })
    expect(full.pos).toBe(8)
  })

  it('refuses a key that joins two matrices', () => {
    expect(planAddedKey(hardware, link(2, 9, false))).toMatchObject({ ok: false })
  })

  it('keeps a key without a diode in a duplex matrix one key, also when the matrix grows', () => {
    const duplex = { ...hardware, matrix: { Matrix: { rows: [2, 3], cols: [4, 5], diodes: 'Both' as const } } }
    const plain = planAddedKey(duplex, link(3, 5, false))
    if (!plain.ok) throw new Error(plain.problem)
    expect([plain.pos, plain.rewired, plain.hardware.doubles]).toEqual([3, true, [7]])
    // Found again it changes nothing.
    expect(planAddedKey(plain.hardware, link(3, 5, false))).toMatchObject({ ok: true, rewired: false, pos: 3 })
    // A new column moves the position that is not reported along with the others.
    const wider = planAddedKey(plain.hardware, link(2, 6))
    if (!wider.ok) throw new Error(wider.problem)
    expect([wider.pos, wider.hardware.doubles]).toEqual([2, [10]])
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
