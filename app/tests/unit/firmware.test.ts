import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { demoById } from '../../src/lib/app'
import { carriedVersion, isOlder, updateDue } from '../../src/lib/firmware'
import { openSession } from '../../src/lib/session'
import { VirtualTransport } from '../../src/lib/transport'
import { loadBridge } from '../../src/lib/wasm'

beforeAll(async () => {
  await loadBridge(readFileSync(new URL('../../src/wasm/jmk.wasm', import.meta.url)))
})

describe('firmware versions', () => {
  it('are compared part by part', () => {
    expect(isOlder([0, 1, 0], [0, 2, 0])).toBe(true)
    expect(isOlder([0, 9, 9], [1, 0, 0])).toBe(true)
    expect(isOlder([0, 2, 0], [0, 2, 0])).toBe(false)
    expect(isOlder([1, 0, 0], [0, 9, 9])).toBe(false)
    expect(isOlder([0, 2, 1], [0, 2, 0])).toBe(false)
  })

  it('a virtual keyboard runs what the app carries and is never offered an update', async () => {
    const demo = demoById('macropad')!
    const transport = new VirtualTransport(demo.name, demo.halves[0].wiring, 201)
    const session = await openSession(transport)
    expect(session.info.firmware).toEqual(carriedVersion())
    expect(updateDue(session)).toBe(false)
    transport.close()
  })
})
