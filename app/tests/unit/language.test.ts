import { afterEach, describe, expect, it } from 'vitest'
import { boardBlocks, charFor, describeAction, EXTRA_KEYS, keyLabel, keyName, legend, textToSteps } from '../../src/lib/keycodes'
import { languageId, LANGUAGES, symbols } from '../../src/lib/language'

afterEach(() => (languageId.value = 'us'))

describe('the language of the computer', () => {
  it('has a character for every key of every language', () => {
    for (const each of LANGUAGES) {
      expect([each.id, each.plain.length, each.shifted.length, each.altgr.length, each.altshift.length]).toEqual([each.id, 48, 48, 48, 48])
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
    // @ needs AltGr; ^ waits for the next key, so it is left out.
    expect(textToSteps('@^{')).toEqual([{ Press: 0xe6 }, { Tap: 0x1f }, { Tap: 0x24 }, { Release: 0xe6 }])
    expect(textToSteps('a b\n')).toEqual([{ Tap: 0x04 }, { Tap: 0x2c }, { Tap: 0x05 }, { Tap: 0x28 }])
    expect([charFor(0x33, false), charFor(0x33, true), charFor(0x2c, false), charFor(0x28, false)]).toEqual(['ö', 'Ö', ' ', undefined])
  })

  it('offers the characters behind Shift and AltGr, and shows them on the key', () => {
    expect(symbols().map((each) => each.char).join('')).toBe('~!@#$%^&*()_+{}:"|<>?')
    expect(legend({ Basic: { ModKey: { mods: 0x02, key: 0x1e } } })).toMatchObject({ main: '!', sub: 'Shift+1' })
    // A capital letter and a shortcut stay what they are.
    expect(legend({ Basic: { ModKey: { mods: 0x02, key: 0x04 } } }).main).toBe('Shift+A')
    expect(legend({ Basic: { ModKey: { mods: 0x01, key: 0x06 } } }).main).toBe('Ctrl+C')
    languageId.value = 'sv'
    const at = symbols().find((each) => each.char === '@')
    expect(at).toEqual({ char: '@', usage: 0x1f, mods: 0x40 })
    expect(symbols().map((each) => each.char)).toEqual(expect.arrayContaining(['{', '}', '[', ']', '\\', '|', '~', '€', '"', '?']))
    expect(legend({ Basic: { ModKey: { mods: 0x40, key: 0x1f } } })).toMatchObject({ main: '@', sub: 'AltGr+2' })
    expect(describeAction({ Basic: { ModKey: { mods: 0x40, key: 0x24 } } })).toBe('{, typed as AltGr + 7')
  })

  it('has the keys only some languages have', () => {
    const usages = EXTRA_KEYS.map((key) => key.usage)
    expect(usages).toEqual(expect.arrayContaining([0x32, 0x64, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x90, 0x91, 0x85, 0x67]))
    expect(keyName(0x89)).toBe('International 3 (Japanese Yen)')
  })

  it('knows that a Mac puts some characters elsewhere', () => {
    languageId.value = 'sv-mac'
    // { is Shift+Option+8 on a Swedish Mac, AltGr+7 on Windows.
    expect(symbols().find((each) => each.char === '{')).toEqual({ char: '{', usage: 0x25, mods: 0x42 })
    expect(legend({ Basic: { ModKey: { mods: 0x42, key: 0x25 } } })).toMatchObject({ main: '{', sub: 'Shift+Option+8' })
    expect(describeAction({ Basic: { ModKey: { mods: 0x40, key: 0x1f } } })).toBe('™, typed as Option + 2')
    expect(textToSteps('{a')).toEqual([{ Press: 0xe1 }, { Press: 0xe6 }, { Tap: 0x25 }, { Release: 0xe1 }, { Release: 0xe6 }, { Tap: 0x04 }])
    // Apple's ISO keyboards swap the keys left of 1 and left of Z.
    expect([keyLabel(0x35), keyLabel(0x64)]).toEqual(['<', '§'])
    languageId.value = 'de-mac'
    expect(symbols().find((each) => each.char === '@')).toEqual({ char: '@', usage: 0x0f, mods: 0x40 })
    expect(symbols().some((each) => each.mods === 0x42)).toBe(true)
  })

  it('has EurKey: a US keyboard with the letters of Western Europe behind AltGr', () => {
    languageId.value = 'eurkey'
    expect([keyLabel(0x33), keyLabel(0x04), boardBlocks()[0].rows[4][0]]).toEqual([';', 'A', [0xe1, 2.25]])
    const find = (char: string) => symbols().find((each) => each.char === char)
    expect([find('ä'), find('Ö'), find('å'), find('ß'), find('€')]).toEqual([
      { char: 'ä', usage: 0x04, mods: 0x40 },
      { char: 'Ö', usage: 0x12, mods: 0x42 },
      { char: 'å', usage: 0x1a, mods: 0x40 },
      { char: 'ß', usage: 0x16, mods: 0x40 },
      { char: '€', usage: 0x22, mods: 0x40 },
    ])
    expect(legend({ Basic: { ModKey: { mods: 0x40, key: 0x04 } } })).toMatchObject({ main: 'ä', sub: 'AltGr+A' })
    expect(textToSteps('ö`')).toEqual([{ Press: 0xe6 }, { Tap: 0x12 }, { Release: 0xe6 }, { Tap: 0x35 }])
  })
})
