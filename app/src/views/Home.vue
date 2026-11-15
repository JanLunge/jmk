<script setup lang="ts">
import { computed } from 'vue'
import KeyboardView from '../components/KeyboardView.vue'
import { app, connectUsb, openDemo } from '../lib/app'
import { DEMOS } from '../lib/demos'
import { usbSupported } from '../lib/transport'

const canUsb = usbSupported()
// Both halves of the split side by side, the right one moved over.
const previews = computed(() =>
  DEMOS.map((demo) => ({
    demo,
    layout: demo.halves.flatMap((half, index) => half.keys.map((key) => ({ ...key, x: key.x + index * 640 }))),
  })),
)
</script>

<template>
  <div class="home scroll">
    <header class="hero col">
      <span class="chip accent">JMK configurator</span>
      <h1>Set up your keyboard<br />without writing a line of code.</h1>
      <p class="muted lead">
        Flash the firmware once. Then tell this app what your board looks like, press every key, and it works out the
        wiring for you.
      </p>
      <div class="row">
        <button class="btn primary" :disabled="!canUsb" data-test="connect" @click="connectUsb">
          Connect a keyboard
        </button>
        <span v-if="!canUsb" class="muted">This browser cannot talk to USB keyboards. Use the desktop app.</span>
        <span v-else-if="app.sessions.length" class="muted">{{ app.sessions.length }} open</span>
      </div>
    </header>

    <section class="col">
      <div>
        <h2>No keyboard yet? Try a virtual one</h2>
        <p class="muted">
          These run the real firmware inside the app. Click their keys as if you were pressing them.
        </p>
      </div>
      <div class="demos">
        <button
          v-for="({ demo, layout }, index) in previews"
          :key="demo.id"
          class="demo card"
          :style="{ animationDelay: index * 70 + 'ms' }"
          :data-test="'demo-' + demo.id"
          @click="openDemo(demo)"
        >
          <div class="preview"><KeyboardView :layout="layout" :max-unit="26" /></div>
          <h3>{{ demo.name }}</h3>
          <p class="muted">{{ demo.blurb }}</p>
        </button>
      </div>
    </section>

    <section class="steps">
      <div v-for="(step, index) in [
        ['Flash', 'Hold the BOOT button while plugging in, then drop the JMK file onto the drive that appears.'],
        ['Describe', 'Pick your controller board and mark any pins used by a display or LEDs.'],
        ['Press', 'Press each key once. The app watches the pins and finds rows, columns and diodes.'],
        ['Make it yours', 'Arrange the keys on screen, choose what each one does, and save.'],
      ]" :key="step[0]" class="step">
        <span class="number">{{ index + 1 }}</span>
        <div>
          <h3>{{ step[0] }}</h3>
          <p class="muted">{{ step[1] }}</p>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.home {
  padding: 56px clamp(24px, 6vw, 80px);
  display: flex;
  flex-direction: column;
  gap: 44px;
}

.hero {
  gap: 16px;
  align-items: flex-start;
  max-width: 640px;
}

.lead {
  font-size: 1.08rem;
}

.demos {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
  gap: 16px;
}

.demo {
  text-align: left;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 6px;
  transition:
    transform 0.2s var(--ease),
    border-color 0.2s,
    box-shadow 0.2s;
  animation: arrive 0.5s var(--ease) backwards;
}

.demo:hover {
  transform: translateY(-4px);
  border-color: var(--accent);
  box-shadow: var(--shadow);
}

.preview {
  height: 120px;
  display: grid;
  place-items: center;
  margin-bottom: 6px;
}

.preview :deep(svg) {
  max-height: 120px;
}

.steps {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
  gap: 22px;
  padding-top: 28px;
  border-top: 1px solid var(--line);
}

.step {
  display: flex;
  gap: 12px;
}

.number {
  flex: none;
  width: 28px;
  height: 28px;
  border-radius: 50%;
  display: grid;
  place-items: center;
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 700;
  font-size: 0.85rem;
}

@keyframes arrive {
  from {
    opacity: 0;
    transform: translateY(18px);
  }
}
</style>
