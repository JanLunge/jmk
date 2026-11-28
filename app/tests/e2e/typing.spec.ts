// Trying a keyboard out from the editor: the field in the header with its
// timing log, and the typing test above the keyboard's layout.
import { expect, test, type Page } from '@playwright/test'
import { assign, hold, openDemo, release, runWizard, saved, tap } from './helpers'

const ms = (text: string | null) => Number(/^(\d+) ms$/.exec(text ?? '')?.[1] ?? NaN)

test('the field in the header shows what a virtual keyboard types and logs every key with its timings', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  await assign(page, 2, 0x2a) // Backspace
  await saved(page)

  await tap(page, 0)
  await tap(page, 1)
  await tap(page, 2)
  await tap(page, 3)
  await expect(page.locator('[data-test=scratch-text]')).toHaveValue('ad')
  // The keys that print nothing are named too.
  await expect(page.locator('[data-test=scratch-keys] .key')).toHaveText(['a', 'b', 'Bksp', 'd'])

  await page.click('[data-test=scratch-keys]')
  const rows = page.locator('[data-test=scratch-log-row]')
  await expect(rows).toHaveCount(4)
  // Newest first; the first key has no key before it to be timed from.
  await expect(rows.locator('.cap')).toHaveText(['d', 'Bksp', 'b', 'a'])
  await expect(rows.nth(3).locator('[data-test=log-gap]')).toHaveText('first')
  for (const row of [0, 1, 2]) {
    // A tap is held about 110 ms and the next one follows about 180 ms after it began.
    const gap = ms(await rows.nth(row).locator('[data-test=log-gap]').textContent())
    const held = ms(await rows.nth(row).locator('[data-test=log-held]').textContent())
    expect(held, `held, row ${row}`).toBeGreaterThan(60)
    expect(held, `held, row ${row}`).toBeLessThan(400)
    expect(gap, `gap, row ${row}`).toBeGreaterThan(held)
    expect(gap, `gap, row ${row}`).toBeLessThan(1500)
  }

  // A key still down says so, and gets its time when it is let go.
  await hold(page, 4)
  await expect(rows.first().locator('[data-test=log-held]')).toHaveText('down')
  await release(page, 4)
  expect(ms(await rows.first().locator('[data-test=log-held]').textContent())).toBeGreaterThan(60)

  await page.click('[data-test=scratch-log-clear]')
  await expect(rows).toHaveCount(0)
  await expect(page.locator('[data-test=scratch-text]')).toHaveValue('')
  // The log is over the page, not in it: the header stays as it was.
  await expect(page.locator('[data-test=nav-keymap]')).toBeVisible()
})

/** The forty-key demo with letters on its first 26 keys, then Space, Backspace and Tab. */
async function fortyReady(page: Page) {
  await openDemo(page, 'forty')
  await runWizard(page)
  await assign(page, 36, 0x2c)
  await assign(page, 37, 0x2a)
  await assign(page, 38, 0x2b)
  await saved(page)
}
const SPACE = 36
const BKSP = 37
const TAB = 38
async function type(page: Page, text: string) {
  for (const char of text) await tap(page, char === ' ' ? SPACE : char.charCodeAt(0) - 97, 50)
}
const word = async (page: Page, index: number) => (await page.locator(`[data-test=test-words] [data-word="${index}"]`).textContent()) ?? ''

test('a timed typing test on a virtual keyboard: words, mistakes, the result and the log', async ({ page }) => {
  test.setTimeout(150_000)
  await fortyReady(page)
  await page.click('[data-test=scratch-expand]')
  await expect(page.locator('[data-test=typing-test]')).toBeVisible()
  // The tabs make room; the editor is one click away.
  await expect(page.locator('[data-test=nav-keymap]')).toHaveCount(0)
  await page.click('[data-test=test-mode-30]')
  await expect(page.locator('[data-test=test-clock]')).toHaveText('30')

  // The key that types the next character is outlined in the layout below.
  const first = await word(page, 0)
  await expect(page.locator('[data-test=test-board] .key.selected')).toHaveAttribute('data-pos', String(first.charCodeAt(0) - 97))

  // Nothing runs until the first key.
  await page.waitForTimeout(1200)
  await expect(page.locator('[data-test=test-clock]')).toHaveText('30')
  await type(page, first + ' ')
  await expect(page.locator('[data-test=test-clock]')).not.toHaveText('30')
  await expect(page.locator('[data-test=test-words] [data-word="0"] .letter.ok')).toHaveCount(first.length)

  // A wrong key shows in the danger colour, and Backspace takes it back.
  const second = await word(page, 1)
  const wrong = second[0] === 'q' ? 'w' : 'q'
  await type(page, wrong)
  await expect(page.locator('[data-test=test-words] [data-word="1"] .letter.bad')).toHaveCount(1)
  await tap(page, BKSP, 50)
  await expect(page.locator('[data-test=test-words] [data-word="1"] .letter.bad')).toHaveCount(0)
  await type(page, second + ' ')
  // A word left wrong stays marked, with what was typed past its end.
  const third = await word(page, 2)
  await type(page, third + 'zz ')
  await expect(page.locator('[data-test=test-words] [data-word="2"]')).toHaveClass(/wrong/)
  await expect(page.locator('[data-test=test-words] [data-word="2"] .letter.extra')).toHaveCount(2)
  await type(page, (await word(page, 3)) + ' ')
  // A key held on the keyboard lights up in the layout.
  await hold(page, 30)
  await expect(page.locator('[data-test=test-board] [data-pos="30"]')).toHaveClass(/pressed/)
  await release(page, 30)

  await expect(page.locator('[data-test=test-result]')).toBeVisible({ timeout: 40_000 })
  // Two right words with their spaces, at the least, in half a minute.
  expect(Number(await page.locator('[data-test=result-wpm]').textContent())).toBeGreaterThanOrEqual(Math.floor(((first.length + second.length + 2) / 5) * 2))
  expect(Number(await page.locator('[data-test=result-raw]').textContent())).toBeGreaterThan(Number(await page.locator('[data-test=result-wpm]').textContent()))
  const accuracy = Number((await page.locator('[data-test=result-accuracy]').textContent())!.replace('%', ''))
  expect(accuracy).toBeGreaterThan(50)
  expect(accuracy).toBeLessThan(100)
  await expect(page.locator('[data-test=speed-chart] path.wpm')).toBeVisible()
  // The key that was due when the wrong one came, and the space the extra letters stood for.
  await expect(page.locator('[data-test=missed-key] .cap')).toContainText([second[0]])
  await expect(page.locator('[data-test=missed-key] .cap')).toContainText(['␣'])

  // The log is reachable from here too, and has every key of the run.
  await page.click('[data-test=scratch-keys]')
  await expect(page.locator('[data-test=scratch-log-row]').first()).toBeVisible()
  expect(await page.locator('[data-test=scratch-log-row]').count()).toBeGreaterThan(first.length + second.length + third.length)
  await expect(page.locator('[data-test=scratch-log-row] .cap').filter({ hasText: 'Bksp' })).toHaveCount(1)
  await page.click('[data-test=scratch-keys]')

  // Tab starts again; the button closes the test and the editor is back.
  await tap(page, TAB, 50)
  await expect(page.locator('[data-test=test-clock]')).toHaveText('30')
  await page.click('[data-test=test-close]')
  await expect(page.locator('[data-test=typing-test]')).toHaveCount(0)
  await expect(page.locator('[data-test=keymap-board]')).toBeVisible()
})

test('a freeform run shows the speed, and the layout follows the keyboard unless a layer is kept in view', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  await page.click('[data-test=add-layer]')
  await assign(page, 1, 0x1e) // layer 1: second key is "1"
  await page.click('[data-test=layer-0]')
  await page.locator('[data-test=keymap-board] [data-pos="0"]').dispatchEvent('pointerdown')
  await page.click('[data-test=tab-layers]')
  await page.click('[data-test=layer-hold-1]')
  await saved(page)

  await page.click('[data-test=scratch-expand]')
  await page.click('[data-test=test-mode-0]')
  for (const key of [1, 2, 3, 4, 5, 1, 2, 3]) await tap(page, key, 50)
  await expect(page.locator('[data-test=test-free]')).toContainText('bcdefbcd')
  await expect(page.locator('[data-test=test-chars]')).toHaveText('8')
  expect(Number(await page.locator('[data-test=test-average]').textContent())).toBeGreaterThan(20)
  expect(Number(await page.locator('[data-test=test-live]').textContent())).toBeGreaterThan(0)

  // The picture shows the base layer, and the layer held on the keyboard while it is held.
  const second = page.locator('[data-test=test-board] [data-pos="1"]')
  await expect(second).toContainText('B')
  await expect(page.locator('[data-test=test-follow]')).toHaveClass(/on/)
  await hold(page, 0)
  await expect(page.locator('[data-test=test-layer-1]')).toHaveClass(/on/)
  await expect(second).toContainText('1')
  await hold(page, 1)
  await expect(second).toHaveClass(/pressed/)
  await release(page, 1)
  await release(page, 0)
  await expect(second).toContainText('B')
  await expect(page.locator('[data-test=test-free]')).toContainText('bcdefbcd1')

  // A layer kept in view stays, whatever the keyboard does; a second click follows again.
  await page.click('[data-test=test-layer-1]')
  await expect(second).toContainText('1')
  await expect(page.locator('[data-test=test-follow]')).not.toHaveClass(/on/)
  await tap(page, 2, 50)
  await expect(second).toContainText('1')
  await page.click('[data-test=test-layer-1]')
  await expect(second).toContainText('B')
  // Nothing here changes the keymap.
  await second.dispatchEvent('pointerdown')
  await expect(page.locator('[data-test=picker]')).toHaveCount(0)
  await expect(page.locator('[data-test=save]')).toHaveText('Saved')
})

test("the computer's own keyboard, as a real keyboard types: the field, its log and the test", async ({ page }) => {
  // The tests have only virtual keyboards; this makes the editor listen to the computer's keys as it does for a real one.
  await page.addInitScript(() => localStorage.setItem('jmk.tryout.keys', 'computer'))
  await openDemo(page, 'macropad')
  await runWizard(page)

  const header = await page.locator('header.bar').boundingBox()
  await page.click('[data-test=scratch-text]')
  await page.keyboard.type('hi', { delay: 40 })
  await page.keyboard.press('Backspace', { delay: 40 })
  await page.keyboard.press('ArrowLeft', { delay: 40 })
  await expect(page.locator('[data-test=scratch-text]')).toHaveValue('h')
  await expect(page.locator('[data-test=scratch-keys] .key')).toHaveText(['h', 'i', 'Bksp', '←'])
  // Using the field moves nothing else in the header.
  expect(await page.locator('header.bar').boundingBox()).toEqual(header)
  await page.click('[data-test=scratch-keys]')
  const rows = page.locator('[data-test=scratch-log-row]')
  await expect(rows.locator('.cap')).toHaveText(['←', 'Bksp', 'i', 'h'])
  expect(ms(await rows.nth(1).locator('[data-test=log-held]').textContent())).toBeGreaterThanOrEqual(30)
  expect(ms(await rows.nth(1).locator('[data-test=log-gap]').textContent())).toBeGreaterThanOrEqual(30)
  await page.keyboard.press('Escape')
  await expect(page.locator('[data-test=scratch-log]')).toHaveCount(0)

  await page.click('[data-test=scratch-expand]')
  await page.click('[data-test=test-mode-0]')
  await expect(page.locator('[data-test=test-input]')).toBeFocused()
  await page.keyboard.type('ok go', { delay: 30 })
  await page.keyboard.press('Backspace')
  await expect(page.locator('[data-test=test-free]')).toContainText('ok g')
  await expect(page.locator('[data-test=test-chars]')).toHaveText('4')

  // Words against the clock, typed right: Tab starts over.
  await page.click('[data-test=test-mode-30]')
  const first = await word(page, 0)
  await page.keyboard.type(first + ' ', { delay: 30 })
  await expect(page.locator('[data-test=test-words] [data-word="0"] .letter.ok')).toHaveCount(first.length)
  await expect(page.locator('[data-test=test-words] [data-word="1"]')).toHaveClass(/current/)
  await page.keyboard.press('Tab')
  await expect(page.locator('[data-test=test-clock]')).toHaveText('30')
  await expect(page.locator('[data-test=test-words] .letter.ok')).toHaveCount(0)

  // Somewhere else in focus: the test says so, and a key brings it back.
  await page.locator('[data-test=test-input]').blur()
  await expect(page.locator('[data-test=test-focus]')).toBeVisible()
  await page.keyboard.type((await word(page, 0))[0])
  await expect(page.locator('[data-test=test-focus]')).toHaveCount(0)
  await expect(page.locator('[data-test=test-words] .letter.ok')).toHaveCount(1)
})
