import { createApp } from 'vue'
import App from './App.vue'
import { app } from './lib/app'
import { loadBridge } from './lib/wasm'
import './style.css'

// Reachable from the developer console and from tests.
;(window as unknown as { jmk: unknown }).jmk = { app }

loadBridge().then(() => createApp(App).mount('#app'))
