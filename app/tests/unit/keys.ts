import type { Action } from '../../src/lib/protocol'

/** A letter, then a digit, per index: handy distinct keys for tests. */
export function starterAction(index: number): Action {
  const usages = [...Array(26).keys()].map((i) => 0x04 + i).concat([...Array(10).keys()].map((i) => 0x1e + i))
  return index < usages.length ? { Basic: { Key: usages[index] } } : { Basic: 'None' }
}
