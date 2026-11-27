// The language the computer's keyboard setting is on.
//
// A keyboard sends positions, not characters: the key right of L is the same
// key code everywhere, and the computer turns it into ; or Ö or Ñ. So this
// changes nothing on the keyboard, only what the keys are called in the app,
// and which keys a macro uses to type a text.

import { ref, watch } from 'vue'

/** The keys whose character depends on the language, row by row as they sit on an ISO keyboard. */
// prettier-ignore
const ORDER = [
  0x35, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2d, 0x2e,
  0x14, 0x1a, 0x08, 0x15, 0x17, 0x1c, 0x18, 0x0c, 0x12, 0x13, 0x2f, 0x30,
  0x04, 0x16, 0x07, 0x09, 0x0a, 0x0b, 0x0d, 0x0e, 0x0f, 0x33, 0x34, 0x31,
  0x64, 0x1d, 0x1b, 0x06, 0x19, 0x05, 0x11, 0x10, 0x36, 0x37, 0x38,
]

export interface Language {
  id: string
  name: string
  /** An ISO keyboard, with one more key next to the left Shift. */
  iso: boolean
  /** What each key of `ORDER` types alone and with Shift. A space stands for nothing. */
  plain: string
  shifted: string
  /** Characters that wait for the next key (accents), so a macro cannot type them in one go. */
  dead: string
}

const rows = (...parts: string[]) => parts.join('')

// Windows and Linux layouts. macOS differs in a few symbols, and on Apple's
// own ISO keyboards the keys left of 1 and left of Z are swapped.
export const LANGUAGES: Language[] = [
  {
    id: 'us', name: 'English (US)', iso: false, dead: '',
    plain: rows('`1234567890-=', 'qwertyuiop[]', "asdfghjkl;'\\", '\\zxcvbnm,./'),
    shifted: rows('~!@#$%^&*()_+', 'QWERTYUIOP{}', 'ASDFGHJKL:"|', '|ZXCVBNM<>?'),
  },
  {
    id: 'uk', name: 'English (UK)', iso: true, dead: '',
    plain: rows('`1234567890-=', 'qwertyuiop[]', "asdfghjkl;'#", '\\zxcvbnm,./'),
    shifted: rows('¬!"£$%^&*()_+', 'QWERTYUIOP{}', 'ASDFGHJKL:@~', '|ZXCVBNM<>?'),
  },
  {
    id: 'sv', name: 'Swedish / Finnish', iso: true, dead: '´`¨^',
    plain: rows('§1234567890+´', 'qwertyuiopå¨', "asdfghjklöä'", '<zxcvbnm,.-'),
    shifted: rows('½!"#¤%&/()=?`', 'QWERTYUIOPÅ^', 'ASDFGHJKLÖÄ*', '>ZXCVBNM;:_'),
  },
  {
    id: 'da', name: 'Danish', iso: true, dead: '´`¨^',
    plain: rows('½1234567890+´', 'qwertyuiopå¨', "asdfghjklæø'", '<zxcvbnm,.-'),
    shifted: rows('§!"#¤%&/()=?`', 'QWERTYUIOPÅ^', 'ASDFGHJKLÆØ*', '>ZXCVBNM;:_'),
  },
  {
    id: 'no', name: 'Norwegian', iso: true, dead: '`¨^',
    plain: rows('|1234567890+\\', 'qwertyuiopå¨', "asdfghjkløæ'", '<zxcvbnm,.-'),
    shifted: rows('§!"#¤%&/()=?`', 'QWERTYUIOPÅ^', 'ASDFGHJKLØÆ*', '>ZXCVBNM;:_'),
  },
  {
    id: 'de', name: 'German', iso: true, dead: '^´`',
    plain: rows('^1234567890ß´', 'qwertzuiopü+', 'asdfghjklöä#', '<yxcvbnm,.-'),
    shifted: rows('°!"§$%&/()=?`', 'QWERTZUIOPÜ*', "ASDFGHJKLÖÄ'", '>YXCVBNM;:_'),
  },
  {
    id: 'fr', name: 'French', iso: true, dead: '^¨',
    plain: rows('²&é"\'(-è_çà)=', 'azertyuiop^$', 'qsdfghjklmù*', '<wxcvbn,;:!'),
    shifted: rows(' 1234567890°+', 'AZERTYUIOP¨£', 'QSDFGHJKLM%µ', '>WXCVBN?./§'),
  },
  {
    id: 'es', name: 'Spanish', iso: true, dead: '`^´¨',
    plain: rows("º1234567890'¡", 'qwertyuiop`+', 'asdfghjklñ´ç', '<zxcvbnm,.-'),
    shifted: rows('ª!"·$%&/()=?¿', 'QWERTYUIOP^*', 'ASDFGHJKLÑ¨Ç', '>ZXCVBNM;:_'),
  },
]

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

/** What a key types in a language, alone and with Shift; undefined for keys that are the same everywhere. */
export function charsOf(usage: number, lang: Language = language()): { plain: string; shifted: string } | undefined {
  const at = position(usage)
  if (at < 0 || (usage === 0x32 && !lang.iso)) return undefined
  const clean = (char: string) => (char === ' ' ? '' : char)
  return { plain: clean(lang.plain[at]), shifted: clean(lang.shifted[at]) }
}

/** The key, and whether Shift is needed, that types a character in the chosen language. */
export function keyFor(char: string, lang: Language = language()): { usage: number; shift: boolean } | undefined {
  if (char === ' ' || lang.dead.includes(char)) return undefined
  const plain = lang.plain.indexOf(char)
  if (plain >= 0) return { usage: ORDER[plain], shift: false }
  const shifted = lang.shifted.indexOf(char)
  return shifted >= 0 ? { usage: ORDER[shifted], shift: true } : undefined
}
