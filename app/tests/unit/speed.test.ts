import { describe, expect, it } from 'vitest'
import { burstWpm, clearSpeed, newSpeed, PAUSE_MS, speedChar, speedStats } from '../../src/lib/speed'

/** Type `count` characters `every` milliseconds apart from `from`; returns when the last one came. */
function type(speed: ReturnType<typeof newSpeed>, from: number, count: number, every: number): number {
  for (let index = 0; index < count; index++) speedChar(speed, from + index * every)
  return from + (count - 1) * every
}

describe('typing speed in bursts', () => {
  it('has no speed before there are enough characters', () => {
    const speed = newSpeed()
    expect(speedStats(speed)).toMatchObject({ last: undefined, best: undefined, typing: undefined, overall: undefined, chars: 0, bursts: 0 })
    type(speed, 0, 4, 100)
    expect(speedStats(speed)).toMatchObject({ last: undefined, overall: undefined, chars: 4, bursts: 0 })
  })

  it('one burst: a character every 200 ms is 60 words a minute', () => {
    const speed = newSpeed()
    type(speed, 1000, 11, 200)
    const stats = speedStats(speed)
    expect(stats.last).toBeCloseTo(60)
    expect(stats.best).toBeCloseTo(60)
    expect(stats.typing).toBeCloseTo(60)
    expect(stats.overall).toBeCloseTo(60)
    expect(stats).toMatchObject({ chars: 11, bursts: 1, typingMs: 2000 })
  })

  it('a pause ends a burst; the pause counts only from start to end', () => {
    const speed = newSpeed()
    const end = type(speed, 0, 11, 200) // 60 wpm for 2 s
    type(speed, end + PAUSE_MS + 1000, 21, 100) // 120 wpm for 2 s, after 3 s of nothing
    const stats = speedStats(speed)
    expect(speed.bursts).toHaveLength(2)
    expect(stats.last).toBeCloseTo(120)
    expect(stats.best).toBeCloseTo(120)
    // 30 intervals in 4 s of typing.
    expect(stats.typing).toBeCloseTo(90)
    // 31 characters after the first in 7 s.
    expect(stats.overall).toBeCloseTo((31 / 5) * (60 / 7))
    expect(stats.bursts).toBe(2)
  })

  it('a gap of exactly the pause still belongs to the burst', () => {
    const speed = newSpeed()
    speedChar(speed, 0)
    speedChar(speed, PAUSE_MS)
    expect(speed.bursts).toHaveLength(1)
    speedChar(speed, 2 * PAUSE_MS + 1)
    expect(speed.bursts).toHaveLength(2)
  })

  it('a few stray characters after a burst do not replace its speed', () => {
    const speed = newSpeed()
    const end = type(speed, 0, 11, 200)
    type(speed, end + 5000, 2, 100)
    expect(speedStats(speed).last).toBeCloseTo(60)
    expect(speedStats(speed).bursts).toBe(1)
    expect(speedStats(speed).chars).toBe(13)
  })

  it('clears', () => {
    const speed = newSpeed()
    type(speed, 0, 10, 100)
    expect(burstWpm(speed.bursts[0])).toBeGreaterThan(0)
    clearSpeed(speed)
    expect(speedStats(speed).chars).toBe(0)
  })
})
