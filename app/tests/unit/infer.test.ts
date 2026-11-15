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

  it('explains a backwards diode and mixed wiring', () => {
    expect(inferWiring([link(2, 5), link(5, 3)])).toMatchObject({ ok: false })
    expect(inferWiring([link(2, 5), { kind: 'ground', pin: 7 }])).toMatchObject({ ok: false })
    expect(inferWiring([link(2, 5, false), link(5, 6, false), link(2, 6, false)])).toMatchObject({ ok: false })
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
