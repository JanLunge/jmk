// A typing test: words to type against the clock, or just typing with the
// speed shown. Plain state and arithmetic; the clock is whatever the caller
// passes in, in milliseconds.

import { pickWords } from './words'

export interface Keystroke {
  at: number
  char: string
  /** The character that was due; none in a freeform run or past the end of a word. */
  expected?: string
  correct: boolean
}

export interface Sample {
  /** The second of the run this is the end of, from 1. */
  second: number
  /** Speed over the whole run so far, counting only what is right. */
  wpm: number
  /** Speed of everything typed within this one second. */
  raw: number
  /** Wrong keys within this second. */
  errors: number
}

export interface TypingTest {
  /** Seconds; 0 for a freeform run, which has no words to type and no end. */
  duration: number
  words: string[]
  /** What was typed for each word reached; the last is the one being typed. A freeform run has one entry, the whole text. */
  typed: string[]
  startedAt?: number
  endedAt?: number
  /** When the last key came. */
  lastAt?: number
  strokes: Keystroke[]
  samples: Sample[]
  seed: number
}

/** Characters typed past the end of a word that are still shown. */
const MAX_EXTRA = 8
const AHEAD = 60

export function newTest(duration: number, seed = Math.floor(Math.random() * 4294967296)): TypingTest {
  const test: TypingTest = { duration, words: [], typed: [''], strokes: [], samples: [], seed }
  if (duration > 0) addWords(test)
  return test
}

function addWords(test: TypingTest): void {
  const more = pickWords(AHEAD, test.seed, test.words[test.words.length - 1])
  test.words.push(...more.words)
  test.seed = more.seed
}

export const isFree = (test: TypingTest) => test.duration === 0
export const isRunning = (test: TypingTest) => test.startedAt !== undefined && test.endedAt === undefined
export const isDone = (test: TypingTest) => test.endedAt !== undefined

/** Let the clock run: takes the samples that are due and ends a timed run. True once it has ended. */
export function tick(test: TypingTest, now: number): boolean {
  if (test.startedAt === undefined || isFree(test)) return false
  if (isDone(test)) return true
  const end = test.startedAt + test.duration * 1000
  while (test.samples.length < test.duration && test.startedAt + (test.samples.length + 1) * 1000 <= now) {
    const second = test.samples.length + 1
    const [from, to] = [test.startedAt + (second - 1) * 1000, test.startedAt + second * 1000]
    // The first key starts the clock, and belongs to the first second.
    const within = test.strokes.filter((stroke) => (second === 1 ? stroke.at >= from : stroke.at > from) && stroke.at <= to)
    test.samples.push({ second, wpm: stats(test, to).wpm, raw: within.length * 12, errors: within.filter((stroke) => !stroke.correct).length })
  }
  if (now >= end) test.endedAt = end
  return isDone(test)
}

/** A character was typed. Space ends a word; in a freeform run it is a character like any other. */
export function typeChar(test: TypingTest, char: string, at: number): void {
  if (isDone(test)) return
  const at0 = test.startedAt
  const last = test.typed.length - 1
  if (isFree(test)) {
    test.startedAt = at0 ?? at
    test.lastAt = at
    test.typed[last] += char
    test.strokes.push({ at, char, correct: true })
    return
  }
  if (char === '\n') return
  // A space before anything of a word is typed is not part of the run.
  if (char === ' ' && !test.typed[last]) return
  if (at0 !== undefined && tick(test, at)) return
  test.startedAt = at0 ?? at
  test.lastAt = at
  const word = test.words[last]
  if (char === ' ') {
    test.strokes.push({ at, char, expected: test.typed[last].length >= word.length ? ' ' : word[test.typed[last].length], correct: test.typed[last] === word })
    test.typed.push('')
    if (test.words.length - test.typed.length < AHEAD / 2) addWords(test)
    return
  }
  if (test.typed[last].length >= word.length + MAX_EXTRA) return
  // Past the end of a word a space was due.
  const expected = word[test.typed[last].length] ?? ' '
  test.typed[last] += char
  test.strokes.push({ at, char, expected, correct: char === expected })
}

/** Take back the last character, or with `word` everything of the word. From the start of a word it goes back into the word before, if that one is wrong. */
export function backspace(test: TypingTest, at: number, word = false): void {
  if (isDone(test) || test.startedAt === undefined) return
  if (!isFree(test) && tick(test, at)) return
  test.lastAt = at
  const last = test.typed.length - 1
  if (isFree(test)) {
    const text = test.typed[last]
    test.typed[last] = word ? text.replace(/\S*\s*$/, '') : [...text].slice(0, -1).join('')
    return
  }
  if (test.typed[last]) {
    test.typed[last] = word ? '' : [...test.typed[last]].slice(0, -1).join('')
    return
  }
  if (last > 0 && test.typed[last - 1] !== test.words[last - 1]) {
    test.typed.pop()
    if (word) test.typed[last - 1] = ''
  }
}

export interface Stats {
  /** Words a minute, a word being five characters; only words typed right count. */
  wpm: number
  /** The same of everything typed, right or wrong. */
  raw: number
  /** Share of the keys pressed that were the right ones, 0 to 100. Mistakes that were corrected still count. */
  accuracy: number
  seconds: number
  correct: number
  incorrect: number
  extra: number
  missed: number
}

/** How long the run has gone on at `now`: for a freeform run up to its last key, as it has no end. */
export function elapsed(test: TypingTest, now: number): number {
  if (test.startedAt === undefined) return 0
  if (isFree(test)) return ((test.lastAt ?? test.startedAt) - test.startedAt) / 1000
  return Math.max(0, Math.min(test.endedAt ?? now, test.startedAt + test.duration * 1000) - test.startedAt) / 1000
}

export function stats(test: TypingTest, now: number): Stats {
  const seconds = elapsed(test, now)
  const out: Stats = { wpm: 0, raw: 0, accuracy: 100, seconds, correct: 0, incorrect: 0, extra: 0, missed: 0 }
  let net = 0
  let gross = 0
  if (isFree(test)) {
    net = gross = out.correct = [...test.typed[0]].length
  } else {
    test.typed.forEach((typed, index) => {
      const word = test.words[index]
      const current = index === test.typed.length - 1
      for (let at = 0; at < Math.min(typed.length, word.length); at++) typed[at] === word[at] ? out.correct++ : out.incorrect++
      out.extra += Math.max(0, typed.length - word.length)
      if (!current) out.missed += Math.max(0, word.length - typed.length)
      // A word counts with the space after it; the one being typed counts as far as it is right.
      if (typed === word) net += word.length + (current ? 0 : 1)
      else if (current && word.startsWith(typed)) net += typed.length
      gross += typed.length + (current ? 0 : 1)
    })
    const right = test.strokes.filter((stroke) => stroke.correct).length
    if (test.strokes.length) out.accuracy = (right / test.strokes.length) * 100
  }
  // The first key starts the clock: before a second has passed a speed means nothing.
  const minutes = Math.max(seconds, 1) / 60
  if (test.startedAt !== undefined) {
    out.wpm = net / 5 / minutes
    out.raw = gross / 5 / minutes
  }
  return out
}

/** The speed right now: what was typed in the last ten seconds. */
export function liveWpm(test: TypingTest, now: number, windowMs = 10_000): number {
  if (test.startedAt === undefined) return 0
  const span = Math.min(windowMs, now - test.startedAt)
  const recent = test.strokes.filter((stroke) => stroke.at > now - windowMs).length
  return recent / 5 / (Math.max(span, 1000) / 60_000)
}

export interface KeyStat {
  char: string
  /** How often it was due. */
  count: number
  /** How often another key was pressed in its place. */
  misses: number
  /** Milliseconds from the key before to this one, on average, when it was right; none when never timed. */
  ms?: number
  timed: number
}

/** A pause longer than this is thinking, or looking for the key on another layer, not the speed of a finger. */
const PAUSE_MS = 2000

/** Per character that was due: how often it was missed and how long it took. */
export function keyStats(test: TypingTest): KeyStat[] {
  const all = new Map<string, KeyStat & { sum: number }>()
  test.strokes.forEach((stroke, index) => {
    if (stroke.expected === undefined) return
    const stat = all.get(stroke.expected) ?? { char: stroke.expected, count: 0, misses: 0, timed: 0, sum: 0 }
    all.set(stroke.expected, stat)
    stat.count++
    if (!stroke.correct) return void stat.misses++
    const before = test.strokes[index - 1]
    // Timed only after a right key: after a mistake the time is spent noticing it.
    if (!before || !before.correct || stroke.at - before.at > PAUSE_MS) return
    stat.timed++
    stat.sum += stroke.at - before.at
  })
  return [...all.values()].map(({ sum, ...stat }) => ({ ...stat, ms: stat.timed ? sum / stat.timed : undefined }))
}

export const missedMost = (keys: KeyStat[], count = 5) =>
  keys.filter((key) => key.misses > 0).sort((a, b) => b.misses - a.misses || b.misses / b.count - a.misses / a.count || a.char.localeCompare(b.char)).slice(0, count)

/** The slowest keys among those timed at least twice, so that one hesitation does not make a key slow. */
export const slowest = (keys: KeyStat[], count = 5) =>
  keys.filter((key) => key.ms !== undefined && key.timed >= 2).sort((a, b) => b.ms! - a.ms!).slice(0, count)
