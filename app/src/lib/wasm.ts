// Loads the Rust bridge (crates/jmk-wasm) and exposes its single entry point.

import wasmUrl from '../wasm/jmk.wasm?url'

interface Exports {
  memory: WebAssembly.Memory
  jmk_alloc(len: number): number
  jmk_free(ptr: number, len: number): void
  jmk_call(ptr: number, len: number): bigint
}

let exports: Exports | undefined

export async function loadBridge(source?: BufferSource): Promise<void> {
  if (exports) return
  const bytes = source ?? (await (await fetch(wasmUrl)).arrayBuffer())
  const { instance } = await WebAssembly.instantiate(bytes, {})
  exports = instance.exports as unknown as Exports
}

const encoder = new TextEncoder()
const decoder = new TextDecoder()

/** Run one bridge command. Throws if the Rust side reports an error. */
export function bridge<T = unknown>(command: Record<string, unknown>): T {
  if (!exports) throw new Error('bridge not loaded')
  const input = encoder.encode(JSON.stringify(command))
  const ptr = exports.jmk_alloc(input.length)
  new Uint8Array(exports.memory.buffer, ptr, input.length).set(input)
  const packed = exports.jmk_call(ptr, input.length)
  exports.jmk_free(ptr, input.length)
  const outPtr = Number(packed >> 32n)
  const outLen = Number(packed & 0xffffffffn)
  const text = decoder.decode(new Uint8Array(exports.memory.buffer, outPtr, outLen))
  exports.jmk_free(outPtr, outLen)
  const reply = JSON.parse(text)
  if (reply && typeof reply === 'object' && 'error' in reply) throw new Error(reply.error)
  return reply as T
}
