<script setup lang="ts">
// The keyboard as a file: save it, or set this keyboard up from one.
import { computed, ref } from 'vue'
import { attempt, partnerOf, toast } from '../lib/app'
import { parseDefinition, positionCount as definitionPositions, type Definition } from '../lib/definition'
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
</script>

<template>
  <div class="share scroll">
    <div class="col left">
      <div class="card col">
        <h2>Keyboard file</h2>
        <p class="muted">
          Everything about this keyboard in one file: wiring, layout, keymap, macros and lights. Keep it as a backup:
          after the keyboard is flashed anew, “Restore from a keyboard file” in the setup puts it all back. Or give it to
          someone with the same build and they skip the setup.
        </p>
        <div class="row wrap">
          <button class="btn" data-test="export" @click="download">Save as file</button>
          <label class="btn">
            Open a file
            <input type="file" accept=".json,application/json" hidden data-test="import" @change="pickFile" />
          </label>
        </div>
      </div>

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
  grid-template-columns: minmax(0, 640px);
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
