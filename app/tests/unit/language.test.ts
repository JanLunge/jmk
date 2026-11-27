import { afterEach, describe, expect, it } from 'vitest'
import { boardBlocks, charFor, keyLabel, keyName, textToSteps } from '../../src/lib/keycodes'
import { languageId, LANGUAGES } from '../../src/lib/language'

afterEach(() => (languageId.value = 'us'))

describe('the language of the computer', () => {
  it('has a character for every key of every language', () => {
    for (const each of LANGUAGES) {
      expect([each.id, [...each.plain].length, [...each.shifted].length]).toEqual([each.id, 48, 48])
    }
  })

  it('leaves US names alone', () => {
    expect([keyLabel(0x33), keyName(0x33), keyLabel(0x04)]).toEqual([';', 'Semicolon', 'A'])
    expect(boardBlocks()[0].rows[4][0]).toEqual([0xe1, 2.25])
  })

  it('names the keys as a Swedish computer types them', () => {
    languageId.value = 'sv'
    expect([keyLabel(0x2f), keyLabel(0x34), keyLabel(0x33)]).toEqual(['Å', 'Ä', 'Ö'])
    expect(keyName(0x33)).toBe('Ö (US: Semicolon)')
    expect(keyName(0x38)).toBe('- and _ (US: Slash)')
    // Unchanged keys keep their names, and the key next to Enter is one key under both codes.
    expect([keyLabel(0x04), keyName(0x28), keyLabel(0x31), keyLabel(0x32)]).toEqual(['A', 'Enter', "'", "'"])
    expect(boardBlocks()[0].rows[4].slice(0, 2)).toEqual([[0xe1, 1.25], [0x64, 1]])
  })

  it('moves Y and Z for German and shows ß', () => {
    languageId.value = 'de'
    expect([keyLabel(0x1c), keyLabel(0x1d), keyLabel(0x2d)]).toEqual(['Z', 'Y', 'ß'])
  })

  it('keeps the digits on the French number row', () => {
    languageId.value = 'fr'
    expect([keyLabel(0x1e), keyLabel(0x14), keyLabel(0x04)]).toEqual(['1', 'A', 'Q'])
    expect(keyName(0x1f)).toBe('é and 2 (US: 2)')
  })

  it('types a text with the keys of the chosen language', () => {
    expect(textToSteps('a;')).toEqual([{ Tap: 0x04 }, { Tap: 0x33 }])
    languageId.value = 'sv'
    expect(textToSteps('åÖ-')).toEqual([{ Tap: 0x2f }, { Press: 0xe1 }, { Tap: 0x33 }, { Release: 0xe1 }, { Tap: 0x38 }])
    // No key for it without AltGr, and ^ waits for the next key.
    expect(textToSteps('@^')).toEqual([])
    expect(textToSteps('a b\n')).toEqual([{ Tap: 0x04 }, { Tap: 0x2c }, { Tap: 0x05 }, { Tap: 0x28 }])
    expect([charFor(0x33, false), charFor(0x33, true), charFor(0x2c, false), charFor(0x28, false)]).toEqual(['ö', 'Ö', ' ', undefined])
  })
})
