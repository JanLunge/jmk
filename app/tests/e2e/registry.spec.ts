// Publishing to and installing from a real registry server.
//   JMK_REGISTRY=http://127.0.0.1:8799 npx playwright test registry
import { expect, test } from '@playwright/test'
import { openDemo, runWizard, tap, typed } from './helpers'

const registry = process.env.JMK_REGISTRY
test.skip(!registry, 'set JMK_REGISTRY to a running registry server')

test('publish a keyboard, then set another one up from the registry', async ({ page }) => {
  const name = `Test pad ${Date.now()}`
  await openDemo(page, 'macropad')
  await runWizard(page)
  await page.locator('[data-test=keymap-board] [data-pos="0"]').dispatchEvent('pointerdown')
  await page.click('.picker [data-usage="29"]') // first key: Z

  await page.click('[data-test=nav-share]')
  await page.fill('[data-test=registry-url]', registry!)
  await page.locator('[data-test=registry-url]').blur()
  await page.fill('[data-test=publish-name]', name)
  await page.click('[data-test=publish]')
  await expect(page.locator('[data-test=published]')).toBeVisible()

  // A second, fresh keyboard of the same build picks it from the list
  // instead of running the wizard.
  await page.click('nav .logo')
  await page.click('[data-test=demo-macropad]')
  await page.click('[data-test=next]')
  await page.evaluate(() => {
    const app = (window as any).jmk.app
    app.sessions[1].setup = false // leave the wizard: this board will be set up from a file
  })
  await page.click('[data-test=nav-share]')
  await page.fill('[data-test=registry-search]', name)
  await page.press('[data-test=registry-search]', 'Enter')
  await page.locator('.result', { hasText: name }).click()
  await page.click('[data-test=apply-definition]')
  await expect(page.getByText(`Set up as ${name}`)).toBeVisible()
  await tap(page, 0)
  await expect(typed(page)).toHaveText('z')
})
