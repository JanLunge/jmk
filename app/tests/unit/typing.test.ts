// Trying a keyboard out: the timing log, the keys read from reports and from
// the browser, and the typing test's arithmetic.
import { afterEach, describe, expect, it } from 'vitest'
import { domStroke, ReportReader, typedChar } from '../../src/lib/keyfeed'
import { clearLog, formatMs, logLoseHeld, logPress, logRelease, newKeyLog } from '../../src/lib/keylog'
import { languageId } from '../../src/lib/language'
import { effective, heldOn, layerLegend, whereIs } from '../../src/lib/layers'
import type { Action, HostEvent } from '../../src/lib/protocol'
import { newTryout, takeStroke } from '../../src/lib/tryout'
import { backspace, elapsed, isDone, keyStats, liveWpm, missedMost, newTest, slowest, stats, tick, typeChar, type TypingTest } from '../../src/lib/typingtest'
import { pickWords, WORDS } from '../../src/lib/words'

afterEach(() => (languageId.value = 'us'))

const report = (keys: number[], mods = 0): HostEvent => ({ Keyboard: { mods, keys } })
const A = 0x04
const key = (usage: number): Action => ({ Basic: { Key: usage } })

describe('the timing log', () => {
  it('times each press from the one before and from its own release', () => {
    const log = newKeyLog()
    logPress(log, 'a', 'a', 1000)
    logPress(log, 'b', 'b', 1042)
    logRelease(log, 'a', 1090)
    logRelease(log, 'b', 1100.4)
    expect(log.entries.map(({ label, gap, held, down }) => ({ label, gap, held, down }))).toEqual([
      { label: 'a', gap: undefined, held: 90, down: false },
      { label: 'b', gap: 42, held: expect.closeTo(58.4), down: false },
    ])
  })

  it('times two presses of one key apart, and a release nobody saw leaves no time', () => {
    const log = newKeyLog()
    logPress(log, 'a', 'a', 0)
    logPress(log, 'a', 'a', 200)
    logRelease(log, 'a', 260)
    expect(log.entries.map((entry) => [entry.held, entry.down])).toEqual([[undefined, false], [60, false]])
    logPress(log, 'b', 'b', 300)
    logLoseHeld(log)
    logRelease(log, 'b', 400)
    expect([log.entries[2].gap, log.entries[2].held, log.entries[2].down]).toEqual([100, undefined, false])
  })

  it('keeps the newest entries and can be cleared', () => {
    const log = newKeyLog(3)
    for (let at = 0; at < 5; at++) logPress(log, `k${at}`, String(at), at * 10)
    expect(log.entries.map((entry) => entry.label)).toEqual(['2', '3', '4'])
    // What fell off the far end is still what the oldest one left was timed from.
    expect(log.entries[0].gap).toBe(10)
    clearLog(log)
    expect(log.entries).toEqual([])
    expect(logPress(log, 'a', 'a', 99).gap).toBeUndefined()
  })

  it('writes milliseconds whole, and seconds once that is easier to read', () => {
    expect([formatMs(41.6), formatMs(0), formatMs(9999), formatMs(12_340), formatMs(undefined)]).toEqual(['42 ms', '0 ms', '9999 ms', '12.3 s', '–'])
  })
})

describe('keys read from a virtual keyboard', () => {
  it('finds presses and releases by comparing each report with the one before', () => {
    const reader = new ReportReader()
    expect(reader.read(report([A]))).toEqual([{ key: 'k4', down: true, label: 'a', kind: 'char', text: 'a', word: false }])
    // A second key while the first is held: only the new one is a press.
    expect(reader.read(report([A, 0x05])).map((stroke) => [stroke.key, stroke.down, stroke.text])).toEqual([['k5', true, 'b']])
    expect(reader.read(report([0x05])).map((stroke) => [stroke.key, stroke.down])).toEqual([['k4', false]])
    expect(reader.read(report([])).map((stroke) => [stroke.key, stroke.down])).toEqual([['k5', false]])
  })

  it('names keys that print nothing, and knows Backspace, Tab, Esc and Enter', () => {
    const reader = new ReportReader()
    const names = [0x2a, 0x2b, 0x29, 0x28, 0x52, 0x2c].map((usage) => {
      const [stroke] = reader.read(report([usage]))
      reader.read(report([]))
      return [stroke.label, stroke.kind, stroke.text]
    })
    expect(names).toEqual([['Bksp', 'backspace', undefined], ['Tab', 'tab', undefined], ['Esc', 'escape', undefined], ['Enter', 'enter', undefined], ['↑', 'other', undefined], ['Space', 'char', ' ']])
  })

  it('puts modifiers down before the key and up after it, and types the shifted character', () => {
    const reader = new ReportReader()
    expect(reader.read(report([0x1e], 0x02)).map((stroke) => [stroke.label, stroke.down, stroke.text])).toEqual([['Shift', true, undefined], ['!', true, '!']])
    expect(reader.read(report([])).map((stroke) => [stroke.label, stroke.down])).toEqual([['1', false], ['Shift', false]])
  })

  it('takes a key with Ctrl as a shortcut, not a character', () => {
    const reader = new ReportReader()
    const strokes = reader.read(report([0x06], 0x01))
    expect(strokes.map((stroke) => [stroke.label, stroke.kind, stroke.text])).toEqual([['Ctrl', 'other', undefined], ['Ctrl + C', 'other', undefined]])
    reader.read(report([]))
    expect(reader.read(report([0x2a], 0x01))[1]).toMatchObject({ kind: 'backspace', word: true })
  })

  it('reads media keys, which come and go one at a time', () => {
    const reader = new ReportReader()
    expect(reader.read({ Consumer: 0xe9 })).toEqual([{ key: 'c233', down: true, label: 'Vol+', kind: 'other' }])
    expect(reader.read({ Consumer: 0 })).toEqual([{ key: 'c233', down: false, label: 'Vol+', kind: 'other' }])
  })

  it('types what the chosen language makes of a key, AltGr included', () => {
    languageId.value = 'de'
    expect([typedChar(0x1c, 0), typedChar(0x1d, 0x02), typedChar(0x14, 0x40), typedChar(0x14, 0x01)]).toEqual(['z', 'Y', '@', undefined])
    const [, stroke] = new ReportReader().read(report([0x14], 0x40))
    expect([stroke.label, stroke.text, stroke.kind]).toEqual(['@', '@', 'char'])
  })
})

describe('keys read from the browser', () => {
  const event = (key: string, code: string, more: Partial<KeyboardEvent> = {}) => ({ key, code, ctrlKey: false, altKey: false, metaKey: false, ...more })

  it('names them as the field in the header always did', () => {
    expect(domStroke(event('Backspace', 'Backspace'), true)).toMatchObject({ key: 'Backspace', label: 'Bksp', kind: 'backspace', word: false })
    expect(domStroke(event(' ', 'Space'), true)).toMatchObject({ label: 'Space', kind: 'char' })
    expect(domStroke(event('c', 'KeyC', { ctrlKey: true }), true)).toMatchObject({ label: 'Ctrl + c', kind: 'other' })
    expect(domStroke(event('Control', 'ControlLeft', { ctrlKey: true }), true).label).toBe('Ctrl')
    expect(domStroke(event('Backspace', 'Backspace', { altKey: true }), true).word).toBe(true)
  })

  it('knows a key by its place, so Shift let go first does not lose the release', () => {
    const log = newKeyLog()
    const down = domStroke(event('A', 'KeyA'), true)
    const up = domStroke(event('a', 'KeyA'), false)
    logPress(log, down.key, down.label, 0)
    logRelease(log, up.key, 80)
    expect(log.entries[0]).toMatchObject({ label: 'A', held: 80 })
  })

  it('names an accent key by its place: it types nothing by itself', () => {
    expect(domStroke(event('Dead', 'BracketLeft'), true)).toMatchObject({ label: 'BracketLeft', kind: 'other' })
  })
})

describe('trying a keyboard out', () => {
  it('logs every key, types into the field for a virtual keyboard and tells the open test', () => {
    const tryout = newTryout()
    const reader = new ReportReader()
    const heard: string[] = []
    const send = (event: HostEvent, at: number) => reader.read(event).forEach((stroke) => takeStroke(tryout, stroke, at, true))
    send(report([A]), 0)
    send(report([]), 70)
    send(report([0x05]), 120)
    send(report([]), 180)
    send(report([0x2a]), 300)
    send(report([]), 350)
    expect(tryout.text).toBe('a')
    expect(tryout.log.entries.map((entry) => [entry.label, entry.gap, entry.held])).toEqual([['a', undefined, 70], ['b', 120, 60], ['Bksp', 180, 50]])
    // With the test open the keys are the test's, and the field keeps what it had.
    tryout.expanded = true
    tryout.hear = (stroke) => stroke.down && heard.push(stroke.label)
    send(report([0x06]), 400)
    expect([tryout.text, heard, tryout.log.entries.length]).toEqual(['a', ['c'], 4])
  })
})

describe('the words', () => {
  it('are a few hundred plain lower-case words, none twice', () => {
    expect(WORDS.length).toBeGreaterThan(300)
    expect(new Set(WORDS).size).toBe(WORDS.length)
    expect(WORDS.every((word) => /^[a-z]+$/.test(word))).toBe(true)
  })

  it('come in the same order for the same seed, never one twice in a row', () => {
    const first = pickWords(200, 7)
    expect(pickWords(200, 7)).toEqual(first)
    expect(pickWords(200, 8).words).not.toEqual(first.words)
    expect(first.words.some((word, index) => word === first.words[index - 1])).toBe(false)
    expect(pickWords(1, 3, pickWords(1, 3).words[0]).words[0]).not.toBe(pickWords(1, 3).words[0])
  })
})

/** A timed test with words of our choosing. */
function testOf(words: string[], duration = 30): TypingTest {
  const test = newTest(duration, 1)
  test.words.splice(0, words.length, ...words)
  return test
}
/** Type text, one character every `step` milliseconds from `from`. Returns when the last one was typed. */
function type(test: TypingTest, text: string, from: number, step = 100): number {
  let at = from
  for (const char of text) {
    typeChar(test, char, at)
    at += step
  }
  return at - step
}

describe('a timed test', () => {
  it('starts with the first key and ends when the time is up', () => {
    const test = testOf(['the', 'cat'])
    expect([test.startedAt, tick(test, 5000)]).toEqual([undefined, false])
    type(test, 'the', 1000)
    expect([test.startedAt, tick(test, 30_999), isDone(test)]).toEqual([1000, false, false])
    expect([tick(test, 31_000), test.endedAt]).toEqual([true, 31_000])
    // Nothing typed after the end counts.
    typeChar(test, ' ', 31_050)
    expect(test.typed).toEqual(['the'])
  })

  it('ends by a key that comes too late, without waiting for the clock to be asked', () => {
    const test = testOf(['the', 'cat'])
    type(test, 'the ', 0)
    typeChar(test, 'c', 30_001)
    expect([isDone(test), test.typed, test.samples.length]).toEqual([true, ['the', ''], 30])
  })

  it('counts speed from right words, raw speed from everything, accuracy from every key', () => {
    const test = testOf(['the', 'cat', 'sat', 'down'], 30)
    // "the " right, "cxt " wrong, "sat " right, then "do" of "down".
    type(test, 'the cxt sat do', 0)
    tick(test, 30_000)
    const result = stats(test, 30_000)
    // 4 + 4 + 2 right characters of right words (with their spaces) in half a minute.
    expect(result.wpm).toBeCloseTo((10 / 5) * 2)
    expect(result.raw).toBeCloseTo((14 / 5) * 2)
    // The x, and the space that ended a wrong word.
    expect(result.accuracy).toBeCloseTo((12 / 14) * 100)
    expect(result).toMatchObject({ correct: 10, incorrect: 1, extra: 0, missed: 0, seconds: 30 })
  })

  it('counts a corrected mistake against accuracy but not against speed', () => {
    const test = testOf(['the', 'cat'])
    type(test, 'thx', 0)
    backspace(test, 300)
    type(test, 'e ', 400)
    expect(test.typed).toEqual(['the', ''])
    tick(test, 30_000)
    expect(stats(test, 30_000).wpm).toBeCloseTo((4 / 5) * 2)
    expect(stats(test, 30_000).accuracy).toBeCloseTo(80)
  })

  it('shows characters typed past a word, up to a point, and counts letters left out', () => {
    const test = testOf(['at', 'home'])
    type(test, 'attttttttttttt', 0)
    expect(test.typed[0]).toBe('at' + 't'.repeat(8))
    type(test, ' ho ', 2000)
    expect(stats(test, 3000)).toMatchObject({ correct: 4, extra: 8, missed: 2 })
  })

  it('ignores a space before a word, and goes back into a wrong word but not a right one', () => {
    const test = testOf(['the', 'cat', 'sat'])
    typeChar(test, ' ', 0)
    expect(test.startedAt).toBeUndefined()
    type(test, 'the  cxt ', 0)
    expect(test.typed).toEqual(['the', 'cxt', ''])
    backspace(test, 2000)
    expect(test.typed).toEqual(['the', 'cxt'])
    backspace(test, 2100, true)
    expect(test.typed).toEqual(['the', ''])
    // The word before is right: there is nothing to mend in it.
    backspace(test, 2200)
    expect(test.typed).toEqual(['the', ''])
  })

  it('takes back a whole word at once into the word before', () => {
    const test = testOf(['the', 'cat'])
    type(test, 'thx ', 0)
    backspace(test, 500, true)
    expect(test.typed).toEqual([''])
  })

  it('never runs out of words', () => {
    const test = newTest(60, 5)
    const before = test.words.length
    for (let word = 0; word < before; word++) type(test, test.words[word] + ' ', word * 10, 1)
    expect(test.words.length).toBeGreaterThan(before)
    expect(newTest(60, 5).words).toEqual(test.words.slice(0, before))
  })

  it('samples the speed once a second: the whole run so far, what fell into that second, and its mistakes', () => {
    const test = testOf(['the', 'cat', 'sat'], 30)
    // Five keys in the first second (0 to 800 ms), then two wrong ones in the third.
    type(test, 'the c', 0, 200)
    type(test, 'xx', 2200, 200)
    tick(test, 3000)
    expect(test.samples.map((sample) => [sample.second, sample.raw, sample.errors])).toEqual([[1, 60, 0], [2, 0, 0], [3, 24, 2]])
    // "the " and the c that begins "cat": five characters in one second.
    expect(test.samples[0].wpm).toBeCloseTo(60)
    // After the mistakes only "the " still counts, over three seconds.
    expect(test.samples[2].wpm).toBeCloseTo((4 / 5) * 20)
    tick(test, 60_000)
    expect(test.samples).toHaveLength(30)
  })

  it('tells how long it has run', () => {
    const test = testOf(['the'])
    expect(elapsed(test, 500)).toBe(0)
    typeChar(test, 't', 1000)
    expect([elapsed(test, 2500), elapsed(test, 99_000)]).toEqual([1.5, 30])
  })
})

describe('the keys of a test', () => {
  it('finds the keys missed most and the slowest ones', () => {
    const test = testOf(['abab', 'abab'], 30)
    // a every time 100 ms after the key before; b 400 ms after, and wrong once.
    let at = 0
    for (const [char, wait] of [['a', 0], ['b', 400], ['a', 100], ['x', 400], ['x', 100], ['x', 100], [' ', 100], ['a', 100], ['b', 400], ['a', 100], ['b', 400]] as const) {
      at += wait
      typeChar(test, char, at)
    }
    const keys = keyStats(test)
    expect(keys.find((each) => each.char === 'b')).toMatchObject({ count: 4, misses: 1, timed: 3, ms: 400 })
    // The first a began the run and has no time; the one after the space does.
    expect(keys.find((each) => each.char === 'a')).toMatchObject({ count: 4, misses: 0, timed: 2, ms: 100 })
    // Typed past the end of the word, where a space was due.
    expect(keys.find((each) => each.char === ' ')).toMatchObject({ count: 3, misses: 3 })
    expect(missedMost(keys).map((each) => each.char)).toEqual([' ', 'b'])
    expect(slowest(keys).map((each) => each.char)).toEqual(['b', 'a'])
  })

  it('does not call a key slow for one long pause', () => {
    const test = testOf(['ab', 'ab'])
    typeChar(test, 'a', 0)
    typeChar(test, 'b', 5000)
    expect(keyStats(test).find((each) => each.char === 'b')).toMatchObject({ timed: 0, ms: undefined })
    expect(slowest(keyStats(test))).toEqual([])
  })
})

describe('a freeform run', () => {
  it('has no words and no end, and keeps what was typed', () => {
    const test = newTest(0)
    expect(test.words).toEqual([])
    type(test, 'hello there\n', 0)
    expect([tick(test, 999_000), isDone(test), test.typed]).toEqual([false, false, ['hello there\n']])
    backspace(test, 2000)
    backspace(test, 2100, true)
    expect(test.typed).toEqual(['hello '])
  })

  it('shows the average up to the last key, and the speed of the last ten seconds', () => {
    const test = newTest(0)
    // 50 characters in five seconds: 120 words a minute.
    const last = type(test, 'x'.repeat(50), 0, 100)
    expect(last).toBe(4900)
    expect(stats(test, 60_000).wpm).toBeCloseTo(50 / 5 / (4.9 / 60))
    expect(liveWpm(test, 5000)).toBeCloseTo(120)
    // A pause does not lower the average, but the speed right now falls to nothing.
    expect(stats(test, 600_000).wpm).toBeCloseTo(50 / 5 / (4.9 / 60))
    expect(liveWpm(test, 20_000)).toBe(0)
  })
})

describe('a layer as it is drawn', () => {
  const NONE: Action = { Basic: 'None' }
  const session = {
    layers: [
      [key(A), key(0x05), key(0x2c), { HoldTap: { tap: { Key: 0x07 }, hold: { MomentaryLayer: 1 }, profile: 0 } } as unknown as Action],
      ['Transparent', key(0x1e), { Basic: { ModKey: { mods: 0x02, key: 0x1e } } }, 'Transparent'] as Action[],
      ['Transparent', NONE, key(0x1f), 'Transparent'] as Action[],
    ],
    layersWith: [0, 0, 0],
    macroNames: [],
  }

  it('lets a see-through key do what the layer under it says', () => {
    expect(effective(session, 1, 0)).toEqual({ action: key(A), layer: 0 })
    expect(effective(session, 1, 1)).toEqual({ action: key(0x1e), layer: 1 })
    expect(layerLegend(session, 1, 0)).toMatchObject({ main: 'A', kind: 'through' })
    expect(layerLegend(session, 0, 0)).toMatchObject({ main: 'A', kind: 'key' })
    expect(layerLegend(session, 0, 9)).toBeUndefined()
  })

  it('tells held keys that do what is drawn from those that used another layer', () => {
    const held = { ...session, pressed: new Set([0, 1]), usedLayer: new Map([[0, 0], [1, 0]]), layerAware: true }
    // Drawn on layer 1: key 0 is see-through there and did use layer 0; key 1 shows layer 1's key but used layer 0.
    expect(heldOn(held, 1)).toEqual({ here: new Set([0]), elsewhere: new Set([1]) })
    expect(heldOn({ ...held, layerAware: false }, 1)).toEqual({ here: new Set([0, 1]), elsewhere: new Set() })
  })

  it('finds the key that types a character: on the layer drawn, else on another', () => {
    expect(whereIs(session, 0, A)).toEqual({ layer: 0, pos: 0 })
    // Through a see-through key, and on the tap of a hold-tap.
    expect(whereIs(session, 1, A)).toEqual({ layer: 1, pos: 0 })
    expect(whereIs(session, 0, 0x07)).toEqual({ layer: 0, pos: 3 })
    // Not on the base layer: the lowest layer that has it.
    expect(whereIs(session, 0, 0x1e)).toEqual({ layer: 1, pos: 1 })
    expect(whereIs(session, 0, 0x1f)).toEqual({ layer: 2, pos: 2 })
    expect(whereIs(session, 0, 0x33)).toBeUndefined()
  })

  it('prefers the key that types a shifted character by itself', () => {
    // "!" is Shift and 1: layer 1 has a key for exactly that next to a plain 1.
    expect(whereIs(session, 1, 0x1e, 0x02)).toEqual({ layer: 1, pos: 2 })
    expect(whereIs(session, 1, 0x1e)).toEqual({ layer: 1, pos: 1 })
  })
})
