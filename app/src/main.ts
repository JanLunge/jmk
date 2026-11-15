import { createApp } from 'vue'
import App from './App.vue'
import { loadBridge } from './lib/wasm'
import './style.css'

loadBridge().then(() => createApp(App).mount('#app'))
