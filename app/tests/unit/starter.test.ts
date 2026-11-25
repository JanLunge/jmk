import { describe, expect, it } from 'vitest'
import { ALPHAS, letterActions, rowFrom, rowLayout, splitRows, usageOf } from '../../src/lib/starter'

describe('the first picture of a keyboard', () => {
  it('cuts the keys into rows where the user marked a row end', () => {
    expect(splitRows(7, [2, 5])).toEqual([[0, 1, 2], [3, 4, 5], [6]])
    // The very last key ends a row anyway.
    expect(splitRows(4, [1, 3])).toEqual([[0, 1], [2, 3]])
    expect(splitRows(3, [])).toEqual([[0, 1, 2]])
  })

  it('draws rows in press order, whatever the wiring calls the keys', () => {
    // Pressed: positions 4, 0, 2 in the first row, then 1 alone. Position 3 has no switch.
    const layout = rowLayout([[4, 0, 2], [1]], 5)
    expect(layout[4]).toMatchObject({ x: 0, y: 0, w: 100 })
    expect(layout[0]).toMatchObject({ x: 100, y: 0 })
    expect(layout[2]).toMatchObject({ x: 200, y: 0 })
    expect(layout[1]).toMatchObject({ x: 0, y: 100 })
    expect(layout[3].w).toBe(0)
    // On the left half of a split the short thumb row sits at the inner edge.
    expect(rowLayout([[4, 0, 2], [1]], 5, 'right')[1]).toMatchObject({ x: 200, y: 100 })
  })

  it('finds the keys to the right of one, and puts letters on them', () => {
    const layout = rowLayout([[0, 1, 2, 3], [4, 5, 6, 7]], 8)
    expect(rowFrom(layout, 1)).toEqual([1, 2, 3])
    expect(rowFrom(layout, 4, 2)).toEqual([4, 5])
    const dvorak = ALPHAS.find((a) => a.id === 'dvorak')!
    const actions = letterActions([[0, 1, 2, 3], [4, 5]], dvorak)
    expect(actions.get(0)).toEqual({ Basic: { Key: 0x34 } }) // '
    expect(actions.get(3)).toEqual({ Basic: { Key: usageOf('p') } })
    expect(actions.get(5)).toEqual({ Basic: { Key: usageOf('o') } })
    expect(actions.has(6)).toBe(false)
  })

  it('knows every arrangement as three rows of ten', () => {
    for (const alphas of ALPHAS) {
      expect(alphas.rows.map((row) => row.length)).toEqual([10, 10, 10])
      expect(new Set(alphas.rows.join('')).size).toBe(30)
    }
    expect(usageOf('a')).toBe(0x04)
    expect(usageOf('z')).toBe(0x1d)
  })
})
