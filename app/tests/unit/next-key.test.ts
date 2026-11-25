import { describe, expect, it } from 'vitest'
import { NO_KEY, nextInRow } from '../../src/lib/layout'

const key = (x: number, y: number, w = 100) => ({ x, y, w, h: 100, r: 0 })

describe('the next key while typing keys in', () => {
  it('goes right along the row, then to the start of the row below', () => {
    // Positions out of order on purpose: the wiring rarely matches the picture.
    const layout = [key(200, 0), key(0, 0), key(100, 0), key(100, 100), key(0, 100)]
    expect(nextInRow(layout, 1)).toBe(2)
    expect(nextInRow(layout, 2)).toBe(0)
    expect(nextInRow(layout, 0)).toBe(4)
    expect(nextInRow(layout, 4)).toBe(3)
    expect(nextInRow(layout, 3)).toBeUndefined()
  })

  it('keeps a staggered column in its row, and crosses the gap between split halves', () => {
    const layout = [key(0, 30), key(100, 0), key(200, 20), key(600, 0), key(0, 130), NO_KEY]
    expect(nextInRow(layout, 0)).toBe(1)
    expect(nextInRow(layout, 1)).toBe(2)
    expect(nextInRow(layout, 2)).toBe(3)
    expect(nextInRow(layout, 3)).toBe(4)
  })
})
