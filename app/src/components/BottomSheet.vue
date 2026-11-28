<script setup lang="ts">
// The panel under the keyboard. It folds down to one bar, so the keyboard has
// the room; a click anywhere on that bar opens it again.
const minimized = defineModel<boolean>('minimized', { required: true })

function open(event: MouseEvent) {
  // What can be used while folded keeps its own click.
  if (minimized.value && !(event.target as Element).closest('button, input, select, label')) minimized.value = false
}
</script>

<template>
  <aside class="sheet card" :class="{ minimized }" :title="minimized ? 'Click to open' : undefined" @click="open">
    <slot />
    <button class="fold" :title="minimized ? 'Open' : 'Minimise, so the keyboard has the room'" :aria-label="minimized ? 'Open the panel' : 'Minimise the panel'" :aria-expanded="!minimized" data-test="sheet-fold" @click="minimized = !minimized">
      <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="minimized ? 'm18 15-6-6-6 6' : 'm6 9 6 6 6-6'" /></svg>
    </button>
  </aside>
</template>

<style scoped>
/* The sheet floats over the page, apart from the keyboard it works on. */
.sheet {
  /* What is in the first line keeps this much room free on the right, for the fold button. */
  --fold-room: 34px;
  position: relative;
  min-height: 0;
  width: min(100%, 1180px);
  justify-self: center;
  padding: 12px 16px 14px;
  background: color-mix(in srgb, var(--panel) 92%, var(--text));
  box-shadow: var(--shadow);
}

.sheet.minimized {
  width: auto;
  max-width: 100%;
  padding: 7px 14px 7px 20px;
  border-radius: 999px;
  cursor: pointer;
  transition: border-color 0.15s;
}

.sheet.minimized:hover {
  border-color: var(--muted);
}

.fold {
  position: absolute;
  top: 16px;
  right: 12px;
  width: 32px;
  height: 32px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--muted);
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}

.minimized .fold {
  top: calc(50% - 16px);
  right: 8px;
}

.fold:hover,
.sheet.minimized:hover .fold {
  background: var(--panel-2);
  color: var(--text);
}

.fold svg {
  width: 17px;
  height: 17px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}
</style>
