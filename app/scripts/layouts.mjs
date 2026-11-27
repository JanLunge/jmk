// Writes src/lib/layouts.ts: what each key types in each language, on Windows
// and Linux and on macOS, taken from the keyboard charts of Unicode CLDR.
//
//   node scripts/layouts.mjs            download the charts and write the file
//   node scripts/layouts.mjs --check    fail if the file differs from the charts
//
// The charts are the last CLDR release that carried per-platform keyboards.
// EurKey is not in them and comes from its Linux definition.
import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const SOURCE = 'https://raw.githubusercontent.com/unicode-org/cldr/release-43/keyboards'
const OUT = fileURLToPath(new URL('../src/lib/layouts.ts', import.meta.url))

/** id, name, CLDR locale. */
const LANGUAGES = [
  ['us', 'English (US)', 'en'],
  ['uk', 'English (UK)', 'en-GB'],
  ['sv', 'Swedish', 'sv'],
  ['fi', 'Finnish', 'fi'],
  ['da', 'Danish', 'da'],
  ['no', 'Norwegian', 'nb'],
  ['de', 'German', 'de'],
  ['fr', 'French', 'fr'],
  ['es', 'Spanish', 'es'],
]
const PLATFORMS = [
  ['pc', 'windows'],
  ['mac', 'osx'],
]

/** ISO 9995 positions in the order of ORDER in src/lib/language.ts. The key next to Enter is C12, or D13 where a chart draws an ANSI board. */
const two = (n) => String(n).padStart(2, '0')
const POSITIONS = [
  ...[...Array(13).keys()].map((n) => [`E${two(n)}`]),
  ...[...Array(12).keys()].map((n) => [`D${two(n + 1)}`]),
  ...[...Array(11).keys()].map((n) => [`C${two(n + 1)}`]),
  ['C12', 'D13'],
  ...[...Array(11).keys()].map((n) => [`B${two(n)}`]),
]

const ENTITIES = { '&lt;': '<', '&gt;': '>', '&amp;': '&', '&apos;': "'", '&quot;': '"' }
const text = (raw) =>
  raw
    .replace(/&(lt|gt|amp|apos|quot);/g, (entity) => ENTITIES[entity])
    .replace(/\\u\{([0-9A-Fa-f]+)\}/g, (_, hex) => String.fromCodePoint(parseInt(hex, 16)))

/** Which layer a keyMap is, from its modifiers: the ones marked optional (caps?) do not matter. */
function layerOf(modifiers) {
  if (modifiers === undefined) return 'plain'
  const NAMES = { shift: 'shifted', altR: 'altgr', opt: 'altgr', 'altR+shift': 'altshift', 'opt+shift': 'altshift' }
  // A keyMap lists every combination that reaches it; the one with the fewest keys held says which layer it is.
  const choices = modifiers
    .split(' ')
    .map((choice) =>
      choice
        .split('+')
        .filter((part) => !part.endsWith('?'))
        // Left or right Shift and Option are the same to a layout; AltGr is only the right Alt.
        .map((part) => part.replace(/^(shift|opt)[LR]$/, '$1'))
        .sort(),
    )
    .sort((one, other) => one.length - other.length)
  return NAMES[choices[0].join('+')]
}

function parse(xml) {
  const layers = { plain: {}, shifted: {}, altgr: {}, altshift: {} }
  for (const [, attributes, body] of xml.matchAll(/<keyMap([^>]*)>([\s\S]*?)<\/keyMap>/g)) {
    const layer = layerOf(attributes.match(/modifiers="([^"]*)"/)?.[1])
    if (!layer) continue
    for (const [, iso, to] of body.matchAll(/<map iso="([^"]+)" to="([^"]*)"/g)) layers[layer][iso] ??= text(to)
  }
  // An accent key: it starts a pair that makes another character.
  const dead = new Set()
  for (const [, from] of xml.matchAll(/<transform from="([^"]+)"/g)) {
    const chars = [...text(from)]
    if (chars.length === 2) dead.add(chars[0])
  }
  const row = (layer) =>
    POSITIONS.map((names) => {
      const char = names.map((name) => layers[layer][name]).find((found) => found !== undefined) ?? ' '
      // One printable character of one UTF-16 unit, or nothing.
      return char.length === 1 && char > ' ' && char !== '\u007f' && char !== ' ' ? char : ' '
    }).join('')
  const table = { plain: row('plain'), shifted: row('shifted'), altgr: row('altgr'), altshift: row('altshift') }
  const used = new Set(Object.values(table).join(''))
  return { ...table, dead: [...dead].filter((char) => used.has(char)).sort().join('') }
}

// --- EurKey, which CLDR does not chart: from the Linux definition of it ------

const EURKEY = 'https://gitlab.freedesktop.org/xkeyboard-config/xkeyboard-config/-/raw/xkeyboard-config-2.41/symbols/eu'

/** X keysym names used by the EurKey definition. Accent keys (dead_*) and Compose are left out: they type nothing alone. */
// prettier-ignore
const KEYSYMS = {
  grave: '`', asciitilde: '~', exclam: '!', at: '@', numbersign: '#', dollar: '$', percent: '%', asciicircum: '^',
  ampersand: '&', asterisk: '*', parenleft: '(', parenright: ')', minus: '-', underscore: '_', equal: '=', plus: '+',
  bracketleft: '[', bracketright: ']', braceleft: '{', braceright: '}', backslash: '\\', bar: '|', semicolon: ';',
  colon: ':', apostrophe: "'", quotedbl: '"', comma: ',', period: '.', slash: '/', less: '<', greater: '>', question: '?',
  exclamdown: '¡', onesuperior: '¹', twosuperior: '²', threesuperior: '³', ordfeminine: 'ª', masculine: 'º',
  sterling: '£', yen: '¥', EuroSign: '€', cent: '¢', doublelowquotemark: '„', singlelowquotemark: '‚',
  leftdoublequotemark: '“', leftsinglequotemark: '‘', rightdoublequotemark: '”', rightsinglequotemark: '’',
  endash: '–', emdash: '—', multiply: '×', division: '÷', guillemotleft: '«', guillemotright: '»', notsign: '¬',
  brokenbar: '¦', degree: '°', periodcentered: '·', questiondown: '¿', section: '§',
  ae: 'æ', AE: 'Æ', aring: 'å', Aring: 'Å', ediaeresis: 'ë', Ediaeresis: 'Ë', yacute: 'ý', Yacute: 'Ý', thorn: 'þ',
  THORN: 'Þ', ydiaeresis: 'ÿ', Ydiaeresis: 'Ÿ', udiaeresis: 'ü', Udiaeresis: 'Ü', idiaeresis: 'ï', Idiaeresis: 'Ï',
  odiaeresis: 'ö', Odiaeresis: 'Ö', oe: 'œ', OE: 'Œ', adiaeresis: 'ä', Adiaeresis: 'Ä', ssharp: 'ß', eth: 'ð', ETH: 'Ð',
  egrave: 'è', Egrave: 'È', eacute: 'é', Eacute: 'É', ugrave: 'ù', Ugrave: 'Ù', uacute: 'ú', Uacute: 'Ú',
  oslash: 'ø', Oslash: 'Ø', agrave: 'à', Agrave: 'À', aacute: 'á', Aacute: 'Á', ccedilla: 'ç', Ccedilla: 'Ç',
  igrave: 'ì', Igrave: 'Ì', iacute: 'í', Iacute: 'Í', ntilde: 'ñ', Ntilde: 'Ñ', ograve: 'ò', Ograve: 'Ò',
  oacute: 'ó', Oacute: 'Ó',
}

function keysym(name) {
  if (/^[A-Za-z0-9]$/.test(name)) return name
  if (name.startsWith('dead_') || name === 'Multi_key') return ' '
  const code = name.match(/^U([0-9A-F]{4})$/)
  if (code) return String.fromCodePoint(parseInt(code[1], 16))
  if (!(name in KEYSYMS)) throw new Error(`EurKey uses a keysym this script does not know: ${name}`)
  return KEYSYMS[name]
}

/** The X name of each key, in the order of POSITIONS. */
const XKB_KEYS = [
  'TLDE', ...[...Array(12).keys()].map((n) => `AE${two(n + 1)}`),
  ...[...Array(12).keys()].map((n) => `AD${two(n + 1)}`),
  ...[...Array(11).keys()].map((n) => `AC${two(n + 1)}`), 'BKSL',
  'LSGT', ...[...Array(10).keys()].map((n) => `AB${two(n + 1)}`),
]

function parseXkb(source) {
  const keys = {}
  for (const [, name, levels] of source.matchAll(/key <(\w+)>\s*\{\[([^\]]*)\]\}/g)) keys[name] = levels.split(',').map((level) => keysym(level.trim()))
  const row = (level) => XKB_KEYS.map((name) => keys[name]?.[level] ?? ' ').join('')
  return { plain: row(0), shifted: row(1), altgr: row(2), altshift: row(3), dead: '' }
}

const entries = []
for (const [platform, folder] of PLATFORMS) {
  for (const [id, name, locale] of LANGUAGES) {
    const url = `${SOURCE}/${folder}/${locale}-t-k0-${folder}.xml`
    const answer = await fetch(url)
    if (!answer.ok) throw new Error(`${url}: ${answer.status}`)
    entries.push({ id: platform === 'pc' ? id : `${id}-mac`, name, platform, iso: id !== 'us', ...parse(await answer.text()) })
  }
}

{
  const answer = await fetch(EURKEY)
  if (!answer.ok) throw new Error(`${EURKEY}: ${answer.status}`)
  const eurkey = parseXkb(await answer.text())
  // The same on every kind of computer, so it is listed with both.
  const at = (platform) => entries.findLastIndex((each) => each.platform === platform) + 1
  entries.splice(at('pc'), 0, { id: 'eurkey', name: 'EurKey', platform: 'pc', iso: false, ...eurkey })
  entries.splice(at('mac'), 0, { id: 'eurkey-mac', name: 'EurKey', platform: 'mac', iso: false, ...eurkey })
}

const body = `// Generated by scripts/layouts.mjs. Do not edit; run the script.
// From the keyboard charts of Unicode CLDR 43 (${SOURCE}),
// data © Unicode, Inc., used under the Unicode License (https://www.unicode.org/license.txt).
// EurKey from xkeyboard-config 2.41 (${EURKEY}),
// layout © 2012 Steffen Bruentjen.

export interface Layout {
  id: string
  name: string
  platform: 'pc' | 'mac'
  /** An ISO keyboard, with one more key next to the left Shift. */
  iso: boolean
  /** What each key types alone, with Shift, with AltGr (Option on a Mac), and with both. A space stands for nothing. */
  plain: string
  shifted: string
  altgr: string
  altshift: string
  /** Characters that wait for the next key (accents). */
  dead: string
}

// prettier-ignore
export const LAYOUTS: Layout[] = ${JSON.stringify(entries, null, 2)}
`

if (process.argv.includes('--check')) {
  if (readFileSync(OUT, 'utf8') !== body) {
    console.error('src/lib/layouts.ts differs from the charts; run node scripts/layouts.mjs')
    process.exit(1)
  }
  console.log('layouts.ts matches the charts')
} else {
  writeFileSync(OUT, body)
  console.log(`wrote ${entries.length} layouts`)
}
