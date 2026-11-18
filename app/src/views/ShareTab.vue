<script setup lang="ts">
// Keyboard files and the online registry.
import { computed, onMounted, ref } from 'vue'
import { attempt, partnerOf, toast } from '../lib/app'
import { parseDefinition, positionCount as definitionPositions, type Definition } from '../lib/definition'
import {
  editToken,
  fetchKeyboard,
  publishKeyboard,
  registryMcu,
  registryUrl,
  searchKeyboards,
  setRegistryUrl,
  type KeyboardSummary,
} from '../lib/registry'
import { applyDefinition, exportDefinition, type Session } from '../lib/session'

const props = defineProps<{ session: Session }>()

const partner = computed(() => partnerOf(props.session))
const definition = () => exportDefinition(props.session, partner.value?.hardware)

// --- Files ---------------------------------------------------------------------
function download() {
  const data = definition()
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
  const link = document.createElement('a')
  link.href = URL.createObjectURL(blob)
  link.download = `${data.name.replace(/[^a-z0-9]+/gi, '-').toLowerCase() || 'keyboard'}.jmk.json`
  link.click()
  URL.revokeObjectURL(link.href)
}

const pending = ref<Definition>()
async function pickFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  try {
    pending.value = parseDefinition(await file.text())
  } catch (error) {
    toast(error instanceof Error ? error.message : String(error), 'error')
  }
}

/** Set this board (and the other half, if open) up from a keyboard file. */
async function use(chosen: Definition) {
  const done = await attempt(async () => {
    await applyDefinition(props.session, chosen, 0)
    if (chosen.halves.length > 1 && partner.value) await applyDefinition(partner.value, chosen, 1)
    return true
  })
  pending.value = undefined
  if (done) toast(`Set up as ${chosen.name}`)
}

// --- Registry ------------------------------------------------------------------
const url = ref(registryUrl())
const query = ref('')
const results = ref<KeyboardSummary[]>()
const searching = ref(false)
const offline = ref<string>()

let searches = 0
async function search() {
  // Only the newest search counts: an answer for an address that has been
  // changed since must not overwrite the one for the new address.
  const mine = ++searches
  searching.value = true
  offline.value = undefined
  let found: KeyboardSummary[] | undefined
  let problem: string | undefined
  try {
    setRegistryUrl(url.value)
    found = await searchKeyboards(query.value, registryMcu(props.session.info.mcu))
  } catch (error) {
    problem = error instanceof Error ? error.message : String(error)
  }
  if (mine !== searches) return
  results.value = found
  offline.value = problem
  searching.value = false
}
onMounted(search)

async function choose(summary: KeyboardSummary) {
  const fetched = await attempt(() => fetchKeyboard(summary.slug))
  if (fetched) pending.value = fetched.definition
}

const form = ref({ name: props.session.meta.name, description: '', author: '', tags: '' })
const publishedSlug = ref<string>()
const mine = computed(() => results.value?.find((r) => r.name === form.value.name && editToken(r.slug)))

async function publish() {
  const tags = form.value.tags.split(/[,\s]+/).map((t) => t.toLowerCase().replace(/[^a-z0-9-]/g, '')).filter(Boolean)
  const details = { name: form.value.name.trim(), description: form.value.description.trim(), author: form.value.author.trim(), tags }
  // The file carries the published name, so whoever installs it sees the same one.
  const slug = await attempt(() => publishKeyboard(details, { ...definition(), name: details.name }, mine.value?.slug))
  if (slug) {
    publishedSlug.value = slug
    toast(mine.value ? 'Published a new version' : 'Published')
    await search()
  }
}
</script>

<template>
  <div class="share scroll">
    <div class="col left">
      <div class="card col">
        <h2>Keyboard file</h2>
        <p class="muted">
          Everything about this keyboard in one file: wiring, layout and keymap. Give it to someone with the same
          build and they skip the setup.
        </p>
        <div class="row wrap">
          <button class="btn" data-test="export" @click="download">Save as file</button>
          <label class="btn">
            Open a file
            <input type="file" accept=".json,application/json" hidden data-test="import" @change="pickFile" />
          </label>
        </div>
      </div>

      <div class="card col">
        <h2>Publish to the registry</h2>
        <p class="muted">Share this keyboard so others can pick it from the list.</p>
        <label class="field">Name<input v-model="form.name" class="input" maxlength="64" data-test="publish-name" /></label>
        <label class="field">Description<textarea v-model="form.description" class="input" rows="2" maxlength="2000" /></label>
        <div class="row">
          <label class="field grow">Your name<input v-model="form.author" class="input" maxlength="64" /></label>
          <label class="field grow">Tags<input v-model="form.tags" class="input" placeholder="macropad, handwired" /></label>
        </div>
        <button class="btn primary" :disabled="!form.name.trim() || !!offline" data-test="publish" @click="publish">
          {{ mine ? 'Publish a new version' : 'Publish' }}
        </button>
        <p v-if="publishedSlug" class="note" data-test="published">
          Published as <span class="mono">{{ publishedSlug }}</span>. This computer keeps the key that lets you update it.
        </p>
      </div>
    </div>

    <div class="card col registry">
      <div class="row">
        <h2 class="grow">Registry</h2>
        <input v-model="url" class="input url mono" data-test="registry-url" @change="search" />
      </div>
      <div class="row">
        <input v-model="query" class="input grow" placeholder="Search keyboards…" data-test="registry-search" @keydown.enter="search" />
        <button class="btn" :disabled="searching" @click="search">Search</button>
      </div>
      <p v-if="offline" class="note warn" data-test="registry-offline">{{ offline }}</p>
      <p v-else-if="results && !results.length" class="muted">Nothing found for this chip.</p>
      <TransitionGroup name="pop" tag="div" class="col results">
        <button v-for="result in results" :key="result.slug" class="result" :data-test="'result-' + result.slug" @click="choose(result)">
          <div class="row">
            <strong>{{ result.name }}</strong>
            <span class="chip">{{ result.key_count }} keys</span>
            <span v-if="result.split" class="chip">split</span>
            <span class="grow" />
            <span class="muted small">v{{ result.version }} · {{ result.downloads }} uses</span>
          </div>
          <div class="muted small">{{ result.description || 'No description' }}<template v-if="result.author"> · by {{ result.author }}</template></div>
        </button>
      </TransitionGroup>
    </div>

    <Transition name="fade">
      <div v-if="pending" class="overlay" @click.self="pending = undefined">
        <div class="card col dialog">
          <h2>Set this keyboard up as “{{ pending.name }}”?</h2>
          <p class="muted">
            {{ definitionPositions(pending.halves) }} key positions{{ pending.halves.length > 1 ? ' across two halves' : '' }},
            {{ pending.keymap.layers.length }} layer{{ pending.keymap.layers.length === 1 ? '' : 's' }}.
            This replaces the wiring, layout and keymap on the keyboard and saves.
          </p>
          <p v-if="pending.halves.length > 1 && !partner" class="note warn">
            Only this half is connected. Connect the other half and apply the file there too.
          </p>
          <div class="row">
            <span class="grow" />
            <button class="btn ghost" @click="pending = undefined">Cancel</button>
            <button class="btn primary" data-test="apply-definition" @click="use(pending)">Set up</button>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.share {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 18px;
  padding: 18px 24px;
  align-items: start;
}

.left {
  gap: 18px;
}

.wrap {
  flex-wrap: wrap;
}

.small {
  font-size: 0.85rem;
}

.url {
  width: 220px;
}

textarea {
  resize: vertical;
}

.results {
  gap: 8px;
}

.result {
  text-align: left;
  padding: 11px 14px;
  border-radius: 11px;
  border: 1px solid var(--line);
  background: var(--panel-2);
  cursor: pointer;
  transition:
    border-color 0.15s,
    transform 0.15s var(--ease);
}

.result:hover {
  border-color: var(--accent);
  transform: translateX(3px);
}

.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  display: grid;
  place-items: center;
  z-index: 40;
}

.dialog {
  width: min(480px, 90vw);
  box-shadow: var(--shadow);
}

@media (max-width: 900px) {
  .share {
    grid-template-columns: 1fr;
  }
}
</style>
