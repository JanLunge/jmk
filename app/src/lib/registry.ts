// Client for the JMK registry server (see server/README.md).

import type { Definition, Keymap } from './definition'
import type { Mcu } from './protocol'

export interface KeyboardSummary {
  slug: string
  name: string
  description: string
  author: string
  tags: string[]
  mcu: string
  key_count: number
  split: boolean
  version: number
  downloads: number
  updated_at: string
}

export interface KeymapSummary {
  id: number
  name: string
  description: string
  author: string
  updated_at: string
}

export interface FirmwareFile {
  name: string
  mcu: string
  version: string
  size: number
  url: string
}

const URL_KEY = 'jmk.registry.url'
const TOKENS_KEY = 'jmk.registry.tokens'
export const DEFAULT_REGISTRY = 'http://127.0.0.1:8787'

export function registryUrl(): string {
  return localStorage.getItem(URL_KEY) || DEFAULT_REGISTRY
}

export function setRegistryUrl(url: string) {
  localStorage.setItem(URL_KEY, url.replace(/\/+$/, ''))
}

/** Edit tokens for what was published from this computer, by slug. */
function tokens(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(TOKENS_KEY) ?? '{}')
  } catch {
    return {}
  }
}

export function editToken(slug: string): string | undefined {
  return tokens()[slug]
}

function rememberToken(slug: string, token: string) {
  localStorage.setItem(TOKENS_KEY, JSON.stringify({ ...tokens(), [slug]: token }))
}

export function registryMcu(mcu: Mcu): string {
  return mcu === 'Simulated' ? 'rp2040' : mcu.toLowerCase()
}

async function call<T>(path: string, init: RequestInit = {}, token?: string): Promise<T> {
  let response: Response
  try {
    response = await fetch(registryUrl() + path, {
      ...init,
      headers: {
        ...(init.body ? { 'Content-Type': 'application/json' } : {}),
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
    })
  } catch {
    throw new Error(`Could not reach the registry at ${registryUrl()}.`)
  }
  if (!response.ok) {
    const body = await response.json().catch(() => undefined)
    throw new Error(body?.error?.message ?? `The registry answered with an error (${response.status}).`)
  }
  return response.status === 204 ? (undefined as T) : response.json()
}

export interface Published {
  name: string
  description: string
  author: string
  tags: string[]
}

export async function searchKeyboards(query: string, mcu?: string): Promise<KeyboardSummary[]> {
  const params = new URLSearchParams({ q: query, per_page: '50' })
  if (mcu) params.set('mcu', mcu)
  const page = await call<{ items: KeyboardSummary[] }>(`/api/v1/keyboards?${params}`)
  return page.items
}

export async function fetchKeyboard(slug: string): Promise<{ summary: KeyboardSummary; definition: Definition }> {
  const data = await call<KeyboardSummary & { definition: Definition }>(`/api/v1/keyboards/${encodeURIComponent(slug)}`)
  return { summary: data, definition: data.definition }
}

/** Publish a new keyboard, or a new version if this computer published it before under `slug`. */
export async function publishKeyboard(details: Published, definition: Definition, slug?: string): Promise<string> {
  const body = JSON.stringify({
    ...details,
    mcu: registryMcu(definition.mcu),
    key_count: definition.layout.filter((key) => key.w > 0).length,
    split: definition.halves.length > 1,
    definition,
  })
  const token = slug && editToken(slug)
  if (slug && token) {
    await call(`/api/v1/keyboards/${encodeURIComponent(slug)}`, { method: 'PUT', body }, token)
    return slug
  }
  const created = await call<{ slug: string; edit_token: string }>('/api/v1/keyboards', { method: 'POST', body })
  rememberToken(created.slug, created.edit_token)
  return created.slug
}

export async function listKeymaps(slug: string): Promise<KeymapSummary[]> {
  const page = await call<{ items: KeymapSummary[] }>(`/api/v1/keyboards/${encodeURIComponent(slug)}/keymaps`)
  return page.items
}

export async function fetchKeymap(id: number): Promise<Keymap> {
  const data = await call<{ definition: Keymap }>(`/api/v1/keymaps/${id}`)
  return data.definition
}

export async function publishKeymap(slug: string, details: Omit<Published, 'tags'>, keymap: Keymap): Promise<void> {
  const body = JSON.stringify({ ...details, definition: { schema: 1, ...keymap } })
  const created = await call<{ id: number; edit_token: string }>(
    `/api/v1/keyboards/${encodeURIComponent(slug)}/keymaps`,
    { method: 'POST', body },
  )
  rememberToken(`keymap:${created.id}`, created.edit_token)
}

export async function listFirmware(): Promise<FirmwareFile[]> {
  const page = await call<{ items: FirmwareFile[] }>('/api/v1/firmware')
  return page.items
}
