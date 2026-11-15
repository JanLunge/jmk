import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  // Relative paths so the build also loads from Electron's app:// scheme.
  base: './',
  plugins: [vue()],
  build: { target: 'es2022' },
  test: { include: ['tests/unit/**/*.test.ts'] },
})
