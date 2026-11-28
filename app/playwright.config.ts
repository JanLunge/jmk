import { defineConfig } from '@playwright/test'

// Another checkout's tests may hold the usual port; E2E_PORT picks a free one.
const port = Number(process.env.E2E_PORT ?? 4173)

export default defineConfig({
  testDir: 'tests/e2e',
  timeout: 60_000,
  // The tests share nothing, but the virtual keyboards keep real time.
  workers: 4,
  use: { baseURL: `http://127.0.0.1:${port}`, viewport: { width: 1280, height: 860 } },
  webServer: {
    command: `npx vite preview --port ${port} --strictPort --host 127.0.0.1`,
    url: `http://127.0.0.1:${port}`,
    // Never a server that is already there: it may show another checkout's
    // build, and the tests would pass or fail on code that is not this one.
    reuseExistingServer: false,
  },
})
