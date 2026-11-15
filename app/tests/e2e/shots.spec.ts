// Not assertions: walks through the app and saves pictures for review.
import { test, type Page } from '@playwright/test'

const dir = process.env.SHOTS ?? 'test-results/shots'
const shot = (page: Page, name: string) => page.screenshot({ path: `${dir}/${name}.png` })

test('walk through the macro pad setup', async ({ page }) => {
  await page.goto('/')
  await page.waitForSelector('[data-test=demo-macropad]')
  await page.waitForTimeout(700)
  await shot(page, '01-home')
  await page.click('[data-test=demo-macropad]')
  await page.waitForTimeout(900)
  await shot(page, '02-learn')
  await page.click('[data-test=next]')
  await page.waitForTimeout(500)
  await shot(page, '03-board')
  await page.click('[data-test=next]')
  await page.waitForTimeout(500)
  await shot(page, '04-shape')
  await page.click('[data-test=next]')
  await page.waitForTimeout(900)
  const keys = page.locator('[data-test=virtual-board] .key')
  for (let i = 0; i < 6; i++) {
    await keys.nth(i).dispatchEvent('pointerdown')
    await page.waitForTimeout(160)
    if (i === 3) await shot(page, '05-keys-pressing')
    await page.dispatchEvent('body', 'pointerup')
    await page.waitForTimeout(120)
  }
  await shot(page, '06-keys-done')
  await page.click('[data-test=next]')
  await page.waitForTimeout(600)
  await shot(page, '07-result')
  await page.click('[data-test=apply]')
  await page.waitForTimeout(900)
  await keys.nth(1).dispatchEvent('pointerdown')
  await page.waitForTimeout(250)
  await shot(page, '08-finish')
})
