import { describe, expect, it } from 'vitest'
import { recordingToSteps, type Stroke } from '../../src/lib/recording'

const CMD = 0xe3
const A = 0x04
const C = 0x06
const down = (usage: number, at: number): Stroke => ({ usage, down: true, at })
const up = (usage: number, at: number): Stroke => ({ usage, down: false, at })

// Cmd+A, a breath, Cmd+C: the way a person plays it.
const selectAndCopy = [down(CMD, 0), down(A, 180), up(A, 260), up(CMD, 300), down(CMD, 900), down(C, 1010), up(C, 1090), up(CMD, 1120)]

describe('a recording as macro steps', () => {
  it('keeps only the order by default: shortcuts stay shortcuts, with no waiting', () => {
    expect(recordingToSteps(selectAndCopy, 'order')).toEqual([
      { Press: CMD },
      { Tap: A },
      { Release: CMD },
      { Press: CMD },
      { Tap: C },
      { Release: CMD },
    ])
  })

  it('puts one even pause before each key when asked, never before a release', () => {
    expect(recordingToSteps(selectAndCopy, 'order', 30)).toEqual([
      { Press: CMD },
      { DelayMs: 30 },
      { Tap: A },
      { Release: CMD },
      { DelayMs: 30 },
      { Press: CMD },
      { DelayMs: 30 },
      { Tap: C },
      { Release: CMD },
    ])
  })

  it('replays as played: every pause and hold, less the time a step takes anyway', () => {
    expect(recordingToSteps([down(A, 0), up(A, 80), down(C, 500), up(C, 503)], 'played')).toEqual([
      { Press: A },
      { DelayMs: 78 },
      { Release: A },
      { DelayMs: 418 },
      { Press: C },
      { Release: C },
    ])
  })

  it('ignores key repeat and stray releases, and lets go of what is still held at the end', () => {
    const strokes = [up(C, 0), down(CMD, 10), down(CMD, 40), down(CMD, 70), down(A, 100)]
    expect(recordingToSteps(strokes, 'order')).toEqual([{ Press: CMD }, { Tap: A }, { Release: CMD }])
  })

  it('caps a very long pause', () => {
    expect(recordingToSteps([down(A, 0), up(A, 200_000)], 'played')[1]).toEqual({ DelayMs: 60_000 })
  })
})
