// The Electron shell. Needs a display, so it only runs when asked:
//   JMK_DESKTOP=1 xvfb-run npx playwright test desktop
// With JMK_REAL=1 it also talks to a real JMK keyboard on USB (read-only).
import { _electron as electron, expect, test } from '@playwright/test'

test.skip(!process.env.JMK_DESKTOP, 'set JMK_DESKTOP=1 to run')

test('desktop app starts and runs a virtual keyboard', async () => {
  const app = await electron.launch({ args: ['.', '--no-sandbox'] })
  const page = await app.firstWindow()
  // A real keyboard on USB opens by itself and comes to the front.
  if (process.env.JMK_REAL) await expect(page.locator('nav .board')).toHaveCount(1, { timeout: 15_000 })
  await page.click('nav .logo')
  await expect(page.locator('[data-test=connect]')).toBeEnabled()
  await page.click('[data-test=demo-macropad]')
  await expect(page.locator('[data-test=next]')).toBeVisible()
  await app.close()
})

test('desktop app reads a real keyboard', async () => {
  test.skip(!process.env.JMK_REAL, 'set JMK_REAL=1 with a keyboard attached')
  const app = await electron.launch({ args: ['.', '--no-sandbox'] })
  const page = await app.firstWindow()
  // Keyboards on USB are opened without asking in the desktop app.
  await expect(page.locator('nav .board')).toHaveCount(1, { timeout: 15_000 })
  await page.click('[data-test=nav-hardware]')
  await expect(page.locator('[data-test=wiring-summary]')).toContainText('Matrix')
  if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/20-real-keyboard.png` })
  await app.close()
})
