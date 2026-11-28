// Words for the typing test: common English ones, all lower case and without
// punctuation, so a run is about the letters. Other languages are not covered yet.

// prettier-ignore
export const WORDS = [...new Set((
  'the be of and a to in he have it that for they with as not on she at by this we you do but from or which one would ' +
  'all will there say who make when can more if no man out other so what time up go about than into could state only ' +
  'new year some take come these know see use get like then first any work now may such give over think most even ' +
  'find day also after way many must look before great back through long where much should well people down own just ' +
  'because good each those feel seem how high too place little world very still nation hand old life tell write ' +
  'become here show house both between need mean call develop under last right move thing general school never same ' +
  'another begin while number part turn real leave might want point form off child few small since against ask late ' +
  'home interest large person end open public follow during present without again hold govern around possible head ' +
  'consider word program problem however lead system set order eye plan run keep face fact group play stand increase ' +
  'early course change help line city put close case force meet once water upon war build hear light unite live ' +
  'every country bring center let side try provide continue name certain power pay result question study woman ' +
  'member until far night always service away report something company week church toward start social room figure ' +
  'nature though young less enough almost read include president nothing yet better big boy cost business value ' +
  'second why clear expect family complete act sense mind experience art next near direct car law industry important ' +
  'girl god several matter usual rather per often kind among white reason action return foot care simple within love ' +
  'human along appear doctor believe speak active student month drive concern best door hope example inform body ' +
  'ever least probable understand reach effect different idea whole control condition field pass fall note special ' +
  'talk particular today measure walk teach low hour type carry rate remain full street easy although record sit ' +
  'determine level local sure receive thus moment spirit train college religion perhaps music grow free cause serve ' +
  'age book board recent sound office cut step class true history position above strong friend necessary add court ' +
  'deal tax support party whether either land material happen education death agree arm mother across quite anything ' +
  'town past view society manage answer break organize half fire lose money stop actual already effort wait ' +
  'department able political learn voice air together shall cover common subject draw short wife treat limit road ' +
  'letter color behind produce send term total university rise century success minute remember purpose test fight ' +
  'watch situation south ago difference stage father table rest bear entire market prepare explain offer plant ' +
  'charge ground west picture hard front lie modern dark surface rule regard dance peace observe future wall farm ' +
  'claim firm operation further pressure property morning amount top outside piece sometimes beauty trade fear ' +
  'demand wonder list accept judge paint mile soon responsible allow secretary heart union slow island enter drink ' +
  'story experiment stay paper space apply decide share desire spend sign therefore various visit supply officer ' +
  'doubt private immediate wish contain feed raise describe ready horse son exist north suggest station effective ' +
  'food deep wide alone character english happy critic unit product respect drop nor fill cold represent sudden ' +
  'basic kill fine trouble mark single press heavy attempt origin standard everything committee moral black red bad ' +
  'earth accord else mere die remark basis except equal east event employ defense smile river improve game detail ' +
  'account cent sort reduce club buy attention ship decision wear inside win suppose ride operate realize sale ' +
  'choose park square vote price district dead foreign window beyond direction strike instead trial practice catch ' +
  'opportunity likely recognize permit serious attack floor association spring lot stock lack hair science relation ' +
  'profession pattern quick medical influence occasion machine compare husband blue international fair especially ' +
  'indeed imagine surprise average official temperature difficult sing hit tree race police touch relative throw ' +
  'quality former pull chance prove argue settle growth date heat save performance count production listen main pick ' +
  'size cool army patient combine summer hall slight command enjoy length proper express health chief evening store ' +
  'language degree lay current gun dog plane page weight warm final garden avoid quiet board'
).split(' '))]

/** A small generator of numbers that gives the same run for the same seed. */
function mulberry(seed: number): () => number {
  let state = seed >>> 0
  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = state
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** Words drawn at random, no word twice in a row. The seed returned continues the run. */
export function pickWords(count: number, seed: number, after?: string): { words: string[]; seed: number } {
  const random = mulberry(seed)
  const words: string[] = []
  let last = after
  while (words.length < count) {
    const word = WORDS[Math.floor(random() * WORDS.length)]
    if (word === last) continue
    words.push(word)
    last = word
  }
  return { words, seed: Math.floor(random() * 4294967296) }
}
