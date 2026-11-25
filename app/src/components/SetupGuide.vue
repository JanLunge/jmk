<script setup lang="ts">
// The way from a bare controller to a working keyboard, kept in view for the
// whole setup as one line of dots. A dot says what its step is when pointed at.
defineProps<{
  steps: string[]
  /** Index of the step in progress; the ones before it are done. */
  current: number
}>()
</script>

<template>
  <ol class="setup-guide" data-test="setup-guide" :aria-label="`Setup, step ${current + 1} of ${steps.length}: ${steps[current]}`">
    <li
      v-for="(step, index) in steps"
      :key="step"
      :class="{ done: index < current, now: index === current }"
      :aria-current="index === current ? 'step' : undefined"
      tabindex="0"
    >
      <span class="dot" />
      <span class="name">{{ step }}</span>
    </li>
  </ol>
</template>

<style scoped>
.setup-guide {
  list-style: none;
  margin: 0;
  padding: 14px 0 0;
  flex: none;
  display: flex;
  justify-content: center;
  align-items: center;
}

li {
  position: relative;
  display: flex;
  align-items: center;
  /* A target larger than the dot, so it is easy to point at. */
  padding: 8px 0;
  outline: none;
}

/* The line to the step before. */
li + li::before {
  content: '';
  width: 26px;
  height: 2px;
  margin: 0 5px;
  border-radius: 1px;
  background: var(--line);
  transition: background 0.3s var(--ease);
}

li.done + li::before {
  background: var(--ok);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 4px;
  background: var(--line);
  transition: all 0.3s var(--ease);
}

li.done .dot {
  background: var(--ok);
}

li.now .dot {
  width: 22px;
  background: var(--accent);
}

li:focus-visible .dot {
  box-shadow: 0 0 0 3px var(--accent-soft);
}

.name {
  position: absolute;
  top: 100%;
  right: 0;
  transform: translate(calc(50% - 4px), -4px);
  padding: 3px 9px;
  border-radius: 7px;
  border: 1px solid var(--line);
  background: var(--panel);
  box-shadow: var(--shadow);
  font-size: 0.78rem;
  white-space: nowrap;
  color: var(--text);
  opacity: 0;
  pointer-events: none;
  transition:
    opacity 0.15s var(--ease),
    transform 0.15s var(--ease);
  z-index: 6;
}

li.now .name {
  transform: translate(calc(50% - 11px), -4px);
}

li:hover .name,
li:focus-visible .name {
  opacity: 1;
  transform: translate(calc(50% - 4px), 0);
}

li.now:hover .name,
li.now:focus-visible .name {
  transform: translate(calc(50% - 11px), 0);
}
</style>
