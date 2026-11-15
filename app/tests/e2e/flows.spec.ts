import { expect, test } from '@playwright/test'
import { assign, chord, hold, openDemo, release, runWizard, tap, typed } from './helpers'

const dockShows = (page: import('@playwright/test').Page, label: string) =>
  expect(page.locator('[data-test=virtual-board] strong')).toHaveText(label)

test('macro pad: setup finds the matrix, keys type, edits are live and survive a restart', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  await expect(page.locator('[data-test=nav-keymap]')).toBeVisible()

  await tap(page, 0)
  await tap(page, 5)
  await expect(typed(page)).toHaveText('af')

  await assign(page, 0, 0x14) // Q
  await tap(page, 0)
  await expect(typed(page)).toHaveText('afq')

  await page.click('[data-test=nav-hardware]')
  await expect(page.locator('[data-test=wiring-summary]')).toContainText('rows GP16, GP17; columns GP18, GP19, GP20')

  await page.click('[data-test=save]')
  await expect(page.locator('[data-test=save]')).toHaveText('Saved')
  // A firmware update restarts the keyboard; saved settings must come back.
  await page.click('[data-test=update-firmware]')
  await expect(page.getByText('update mode')).toBeVisible()
  await expect(page.locator('[data-test=update-firmware]')).toBeVisible({ timeout: 10_000 })
  await tap(page, 0)
  await expect(typed(page)).toHaveText('afqq')
})

test('layers and hold-tap', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)

  await page.click('[data-test=add-layer]')
  await expect(page.locator('[data-test=layer-1]')).toHaveClass(/on/)
  await assign(page, 1, 0x1e) // layer 1: second key is "1"
  await page.click('[data-test=layer-0]')
  await page.locator('[data-test=keymap-board] [data-pos="0"]').dispatchEvent('pointerdown')
  await page.click('[data-test=tab-layers]')
  await page.click('[data-test=layer-hold-1]')

  await tap(page, 1) // base layer: b
  await hold(page, 0)
  await tap(page, 1) // with layer 1 held: 1
  await tap(page, 2) // see-through on layer 1: c
  await release(page, 0)
  await expect(typed(page)).toHaveText('b1c')

  // Third key: C when tapped, Shift when held.
  await page.locator('[data-test=keymap-board] [data-pos="2"]').dispatchEvent('pointerdown')
  await page.check('[data-test=hold-tap]')
  await expect(page.locator('[data-test=action-description]')).toContainText('C when tapped, Shift when held')
  await tap(page, 2, 60)
  await hold(page, 2)
  await page.waitForTimeout(320) // past the tapping term
  await tap(page, 1)
  await release(page, 2)
  await expect(typed(page)).toHaveText('b1ccB')
})

test('knob pad: direct wiring and a rotary encoder', async ({ page }) => {
  await openDemo(page, 'knobpad')
  await runWizard(page, { knob: true })
  await page.click('[data-test=nav-hardware]')
  await expect(page.locator('[data-test=wiring-summary]')).toContainText('4 keys, each on its own pin (GP2, GP3, GP4, GP5)')

  await page.click('[data-test=nav-keymap]')
  await page.click('[data-test=knob-0-cw]')
  await page.click('.picker [data-usage="27"]') // clockwise: X
  await page.click('[data-test=knob-0-ccw]')
  await page.click('.picker [data-usage="29"]') // counter-clockwise: Z
  await page.click('[data-test=knob-cw-0]')
  await page.click('[data-test=knob-ccw-0]')
  await expect(typed(page)).toHaveText(/^(xz|zx)$/)
  // Which way is clockwise depends on how the knob was soldered; one click fixes it.
  const before = await typed(page).textContent()
  await page.click('[data-test=nav-hardware]')
  await page.click('[data-test=reverse-0]')
  await page.click('[data-test=knob-cw-0]')
  await expect(typed(page)).toHaveText(before! + before![1])
})

test('split keyboard: both halves set up, keys of the other half arrive over the cable', async ({ page }) => {
  await openDemo(page, 'split')
  await runWizard(page, { shape: 'central', finish: 'finish-partner' })
  await runWizard(page, { shape: 'peripheral' })
  await expect(page.locator('[data-test=peripheral-tester]')).toBeVisible()

  // Back on the main half the other half's keys appear by themselves.
  await page.click('[data-test=other-half]')
  await expect(page.locator('[data-test=split-status]')).toContainText('halves connected')
  await expect(page.locator('[data-test=keymap-board] .key')).toHaveCount(34, { timeout: 8000 })

  await tap(page, 0)
  await page.click('[data-test=other-half]')
  await dockShows(page, 'Right half')
  await tap(page, 0)
  await page.click('[data-test=other-half]')
  await dockShows(page, 'Left half')
  // Left key 0 is "a"; the right half's first key continues the alphabet after 17 left keys.
  await expect(typed(page)).toHaveText('ar')

  await page.click('[data-test=cable]')
  await expect(page.locator('[data-test=split-status]')).toContainText('not heard', { timeout: 6000 })
})

test('layout editor: drag, type exact values, import from keyboard-layout-editor', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  await page.click('[data-test=nav-layout]')
  const key = page.locator('[data-test=layout-canvas] [data-pos="0"]')
  const box = (await key.boundingBox())!
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  await page.mouse.down()
  await page.mouse.move(box.x + box.width / 2 - box.width, box.y + box.height / 2, { steps: 4 })
  await page.mouse.up()
  await expect(page.locator('[data-test=field-x]')).toHaveValue('-1')
  await page.fill('[data-test=field-w]', '2')
  await page.locator('[data-test=field-w]').blur()
  await page.click('[data-test=layout-apply]')
  await page.click('[data-test=nav-keymap]')
  await expect(page.locator('[data-test=keymap-board] .key')).toHaveCount(6)
  const widths = await page.locator('[data-test=keymap-board] .key .base').evaluateAll((rects) => rects.map((r) => r.getAttribute('width')))
  expect(widths.filter((w) => w === '194')).toHaveLength(1)

  await page.click('[data-test=nav-layout]')
  await page.click('[data-test=import-kle]')
  await page.fill('[data-test=kle-text]', '["A","B","C","D","E","F"]')
  await page.click('[data-test=kle-apply]')
  await page.click('[data-test=layout-apply]')
  await page.click('[data-test=nav-keymap]')
  await expect(page.locator('[data-test=keymap-board] .key')).toHaveCount(6)
  const ys = await page.locator('[data-test=keymap-board] .key .base').evaluateAll((rects) => new Set(rects.map((r) => r.getAttribute('y'))).size)
  expect(ys).toBe(1)
})

test('macros and combos', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  await page.click('[data-test=nav-behaviours]')
  await page.click('[data-test=section-macros]')
  await page.fill('[data-test=macro-text]', 'Hi!')
  await page.click('[data-test=macro-add-text]')
  await expect(page.locator('[data-test=macro-steps] li')).toHaveCount(7)

  await page.click('[data-test=section-combos]')
  await page.click('[data-test=add-combo]')
  await page.locator('[data-test=combo-board] [data-pos="1"]').dispatchEvent('pointerdown')
  await page.locator('[data-test=combo-board] [data-pos="2"]').dispatchEvent('pointerdown')
  await page.click('.picker [data-usage="29"]') // Z

  await page.click('[data-test=nav-keymap]')
  await page.locator('[data-test=keymap-board] [data-pos="0"]').dispatchEvent('pointerdown')
  await page.click('[data-test=tab-macros]')
  await page.click('[data-test=macro-0]')
  await tap(page, 0)
  await expect(typed(page)).toHaveText('Hi!')

  await chord(page, [1, 2])
  await expect(typed(page)).toHaveText('Hi!z')
  // One of the two alone is still its own key.
  await tap(page, 1)
  await expect(typed(page)).toHaveText('Hi!zb')
})

test('forty keys: a full hand-wired matrix is found', async ({ page }) => {
  test.setTimeout(90_000)
  await openDemo(page, 'forty')
  await runWizard(page)
  await page.click('[data-test=nav-hardware]')
  await expect(page.locator('[data-test=wiring-summary]')).toContainText('rows GP2, GP3, GP4, GP5; columns GP6')
  await expect(page.locator('.bar .chip').first()).toHaveText('40 keys')
})
