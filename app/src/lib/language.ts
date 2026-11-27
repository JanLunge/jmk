// The language the computer's keyboard setting is on.
//
// A keyboard sends positions, not characters: the key right of L is the same
// key code everywhere, and the computer turns it into ; or Ö or Ñ. So this
// changes nothing on the keyboard, only what the keys are called in the app,
// and which keys a macro uses to type a text.

import { ref, watch } from 'vue'
import { LAYOUTS, type Layout } from './layouts'

/** The keys whose character depends on the language, row by row as they sit on an ISO keyboard. */
// prettier-ignore
const ORDER = [
  0x35, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2d, 0x2e,
  0x14, 0x1a, 0x08, 0x15, 0x17, 0x1c, 0x18, 0x0c, 0x12, 0x13, 0x2f, 0x30,
  0x04, 0x16, 0x07, 0x09, 0x0a, 0x0b, 0x0d, 0x0e, 0x0f, 0x33, 0x34, 0x31,
  0x64, 0x1d, 0x1b, 0x06, 0x19, 0x05, 0x11, 0x10, 0x36, 0x37, 0x38,
]

export type Language = Layout

/**
 * Every language on both kinds of computer. The tables come from the keyboard
 * charts of Unicode CLDR (see scripts/layouts.mjs): Windows and Linux agree
 * with each other, and macOS puts some characters elsewhere.
 */
export const LANGUAGES: Language[] = LAYOUTS

const KEY = 'jmk.language'
function kept(): string {
  try {
    return localStorage.getItem(KEY) ?? 'us'
  } catch {
    return 'us'
  }
}

/** The id of the chosen language. */
export const languageId = ref(LANGUAGES.some((each) => each.id === kept()) ? kept() : 'us')
watch(languageId, (id) => {
  try {
    localStorage.setItem(KEY, id)
  } catch {
    // Not kept, then; the choice still holds while the app is open.
  }
})

export const US = LANGUAGES[0]
export const language = (): Language => LANGUAGES.find((each) => each.id === languageId.value) ?? US

/** The ISO key next to Enter is one key to the computer, whichever of its two codes the keyboard sends. */
const position = (usage: number) => ORDER.indexOf(usage === 0x32 ? 0x31 : usage)

const SHIFT_BIT = 0x02
/** AltGr is the right Alt key; on a Mac it is Option. */
export const ALTGR_BIT = 0x40

const LAYERS = [
  ['plain', 0],
  ['shifted', SHIFT_BIT],
  ['altgr', ALTGR_BIT],
  ['altshift', ALTGR_BIT | SHIFT_BIT],
] as const

/** What the modifiers of a layer are called on the chosen kind of computer. */
export function heldName(mods: number, lang: Language = language()): string | undefined {
  const alt = lang.platform === 'mac' ? 'Option' : 'AltGr'
  return mods === ALTGR_BIT ? alt : mods === (ALTGR_BIT | SHIFT_BIT) ? `Shift+${alt}` : undefined
}

export interface Chars {
  plain: string
  shifted: string
  altgr: string
  altshift: string
}

/** What a key types in a language on each layer; undefined for keys that are the same everywhere. */
export function charsOf(usage: number, lang: Language = language()): Chars | undefined {
  const at = position(usage)
  if (at < 0 || (usage === 0x32 && !lang.iso)) return undefined
  const clean = (char: string) => (char === ' ' ? '' : char)
  return { plain: clean(lang.plain[at]), shifted: clean(lang.shifted[at]), altgr: clean(lang.altgr[at]), altshift: clean(lang.altshift[at]) }
}

/** The key, and the modifiers it needs, that types a character in the chosen language. */
export function keyFor(char: string, lang: Language = language()): { usage: number; mods: number } | undefined {
  if (char === ' ' || lang.dead.includes(char)) return undefined
  for (const [layer, mods] of LAYERS) {
    const at = lang[layer].indexOf(char)
    if (at >= 0) return { usage: ORDER[at], mods }
  }
  return undefined
}

export interface Symbol {
  char: string
  usage: number
  mods: number
}

/**
 * Every character of the chosen language that needs Shift, AltGr or both and
 * is not a letter the keys already show: the ones nobody finds by looking.
 */
export function symbols(lang: Language = language()): Symbol[] {
  const all: Symbol[] = []
  for (const [layer, mods] of LAYERS.slice(1)) {
    ;[...lang[layer]].forEach((char, at) => {
      const capital = char.toLowerCase() !== char && lang.plain[at] === char.toLowerCase()
      if (char !== ' ' && !capital && !lang.plain.includes(char) && !all.some((each) => each.char === char)) all.push({ char, usage: ORDER[at], mods })
    })
  }
  return all
}
