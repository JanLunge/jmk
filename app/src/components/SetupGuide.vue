<script setup lang="ts">
// The way from a bare controller to a working keyboard, kept in view for the
// whole setup: which step this is, and one thing worth knowing about it.
defineProps<{
  steps: string[]
  /** Index of the step in progress; the ones before it are done. */
  current: number
  tip?: { title: string; text: string }
}>()
</script>

<template>
  <aside class="setup-guide col" data-test="setup-guide">
    <h4>Setup</h4>
    <ol>
      <li v-for="(step, index) in steps" :key="step" :class="{ done: index < current, now: index === current }">
        <span class="mark">{{ index < current ? '✓' : '' }}</span>{{ step }}
      </li>
    </ol>
    <Transition name="fade" mode="out-in">
      <div v-if="tip" :key="tip.text" class="tip" data-test="guide-tip">
        <strong>{{ tip.title }}</strong>
        {{ tip.text }}
      </div>
    </Transition>
  </aside>
</template>

<style scoped>
.setup-guide {
  width: 210px;
  flex: none;
  padding: 22px 16px 16px;
  border-right: 1px solid var(--line);
  background: var(--panel);
  gap: 10px;
}

h4 {
  margin: 0;
  font-size: 0.72rem;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--muted);
  font-weight: 600;
}

ol {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 9px;
}

li {
  display: flex;
  gap: 9px;
  align-items: center;
  color: var(--muted);
  transition: color 0.3s var(--ease);
}

.mark {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 1.5px solid var(--line);
  flex: none;
  display: grid;
  place-items: center;
  font-size: 10px;
  font-weight: 700;
  color: var(--accent-ink);
  transition: all 0.3s var(--ease);
}

li.done {
  color: var(--text);
}

li.done .mark {
  background: var(--ok);
  border-color: var(--ok);
}

li.now {
  color: var(--text);
  font-weight: 600;
}

li.now .mark {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-soft);
}

.tip {
  margin-top: auto;
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 9px 11px;
  font-size: 0.82rem;
  color: var(--muted);
  background: var(--bg);
}

.tip strong {
  display: block;
  color: var(--accent);
  font-size: 0.7rem;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  margin-bottom: 2px;
}

@media (max-width: 860px) {
  .setup-guide {
    display: none;
  }
}
</style>
