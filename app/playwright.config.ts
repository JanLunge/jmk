import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: 'tests/e2e',
  timeout: 60_000,
  // The tests share nothing, but the virtual keyboards keep real time.
  workers: 4,
  use: { baseURL: 'http://127.0.0.1:4173', viewport: { width: 1280, height: 860 } },
  webServer: {
    command: 'npm run preview -- --host 127.0.0.1',
    url: 'http://127.0.0.1:4173',
    reuseExistingServer: true,
  },
})
