import { expect, test } from '@playwright/test'
import { assign, chord, hold, openDemo, openShape, release, runWizard, tap, typed, virtualKeys } from './helpers'

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
  await expect(page.locator('[data-test=layer-1]')).toHaveClass(/live/)
  await hold(page, 1) // with layer 1 held: 1
  // Layer 0 is on screen, where this key says B: it is marked only faintly.
  const second = page.locator('[data-test=keymap-board] [data-pos="1"]')
  await expect(second).toHaveClass(/elsewhere/)
  await expect(second).not.toHaveClass(/pressed/)
  await expect(page.locator('[data-test=keymap-board] [data-pos="0"]')).toHaveClass(/pressed/)
  await page.click('[data-test=layer-1]')
  await expect(second).toHaveClass(/pressed/)
  await page.click('[data-test=layer-0]')
  await release(page, 1)
  await expect(page.locator('[data-test=keymap-board] .elsewhere')).toHaveCount(0)
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
  // The knob sits in the picture: its right half is clockwise, its left half the other way.
  await expect(page.locator('[data-test=keymap-board] [data-knob="4"]')).toBeVisible()
  await assign(page, 5, 27) // clockwise: X
  await assign(page, 4, 29) // counter-clockwise: Z
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

test('typing on the computer keyboard fills the keymap key by key', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  await page.locator('[data-test=keymap-board] [data-pos="0"]').dispatchEvent('pointerdown')
  await page.keyboard.press('q')
  await page.keyboard.press('Shift+1')
  await page.keyboard.press('Escape')
  // Three keys filled, and the fourth is now selected.
  await expect(page.locator('[data-test=keymap-board] [data-pos="3"]')).toHaveClass(/selected/)
  await page.waitForTimeout(150)
  await tap(page, 0)
  await tap(page, 1)
  await expect(typed(page)).toHaveText('q!')
  await page.locator('[data-test=keymap-board] [data-pos="2"]').dispatchEvent('pointerdown')
  await expect(page.locator('[data-test=action-description]')).toContainText('Esc')

  // Switched off, typing changes nothing.
  await page.uncheck('[data-test=typing]')
  await page.keyboard.press('z')
  await page.waitForTimeout(150)
  await expect(page.locator('[data-test=action-description]')).toContainText('Esc')
})

test('a key skipped during setup is added afterwards', async ({ page }) => {
  await openDemo(page, 'macropad')
  // The wizard, pressing every key but the last.
  await expect(page.locator('[data-test=found]')).toBeVisible()
  await page.waitForTimeout(750)
  for (let key = 0; key < 5; key++) await tap(page, key)
  await expect(page.locator('[data-test=count]')).toContainText('5 keys')
  await page.click('[data-test=next]')
  await page.click('[data-test=finish]')
  await expect(page.locator('.bar .chip').first()).toHaveText('5 keys')

  await page.click('[data-test=nav-hardware]')
  await page.click('[data-test=add-key]')
  await expect(page.locator('[data-test=add-key-listening]')).toBeVisible()
  await page.waitForTimeout(750)
  await tap(page, 0) // already known
  await expect(page.locator('[data-test=add-key-problem]')).toContainText('already part')
  await tap(page, 5)
  await expect(page.locator('.bar .chip').first()).toHaveText('6 keys')
  await expect(page.locator('[data-test=add-key]')).toBeVisible()
  await tap(page, 5)
  await tap(page, 0)
  await expect(typed(page)).toHaveText('fa')
})

test('layout editor: a rectangle selects several keys to move and resize together', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  await page.click('[data-test=nav-layout]')
  const canvas = page.locator('[data-test=layout-canvas]')
  const boxOf = async (pos: number) => (await canvas.locator(`[data-pos="${pos}"] rect`).boundingBox())!
  const first = await boxOf(0)
  const second = await boxOf(1)
  // From the empty space above-left of the first key to the middle of the second.
  await page.mouse.move(first.x - 30, first.y - 30)
  await page.mouse.down()
  await page.mouse.move(second.x + second.width / 2, second.y + second.height / 2, { steps: 5 })
  await expect(page.locator('[data-test=marquee]')).toBeVisible()
  await page.mouse.up()
  await expect(page.locator('[data-test=marquee]')).toHaveCount(0)
  await expect(canvas.locator('.key.selected')).toHaveCount(2)
  await expect(page.locator('.side h3').first()).toHaveText('2 keys')

  // One width for both; the untouched third key keeps its own.
  await page.fill('[data-test=field-w]', '1.5')
  await page.locator('[data-test=field-w]').blur()
  expect(Math.round(((await boxOf(1)).width / (await boxOf(2)).width) * 100)).toBeGreaterThan(140)
  // The row made room: each key starts where the one before it ends, the unselected third included.
  const row = [await boxOf(0), await boxOf(1), await boxOf(2)]
  const unit = row[2].width
  expect(Math.abs(row[1].x - (row[0].x + row[0].width))).toBeLessThan(unit * 0.15)
  expect(Math.abs(row[2].x - (row[1].x + row[1].width))).toBeLessThan(unit * 0.15)
  // Narrower again: the keys sit against each other, so they close up.
  await page.fill('[data-test=field-w]', '1')
  await page.locator('[data-test=field-w]').blur()
  const closed = [await boxOf(0), await boxOf(1), await boxOf(2)]
  expect(Math.abs(closed[1].x - (closed[0].x + closed[0].width))).toBeLessThan(unit * 0.15)
  expect(Math.abs(closed[2].x - (closed[1].x + closed[1].width))).toBeLessThan(unit * 0.15)

  // One key alone does the same. Positions are read from the side panel,
  // in key units, because the picture rescales after every change.
  const select = async (pos: number) => {
    await page.keyboard.press('Escape') // a click on a selected key keeps the whole selection
    await canvas.locator(`[data-pos="${pos}"]`).dispatchEvent('pointerdown')
    await page.dispatchEvent('body', 'pointerup')
  }
  const set = async (pos: number, field: string, value: number) => {
    await select(pos)
    await page.fill(`[data-test=field-${field}]`, String(value))
    await page.locator(`[data-test=field-${field}]`).blur()
  }
  const lefts = async () => {
    const values: number[] = []
    for (const pos of [0, 1, 2]) {
      await select(pos)
      values.push(Number(await page.inputValue('[data-test=field-x]')))
    }
    return values.map((value) => value - values[0])
  }
  expect(await lefts()).toEqual([0, 1, 2])
  // Narrower: the keys sitting against it follow, one after the other.
  await set(0, 'w', 0.5)
  expect(await lefts()).toEqual([0, 0.5, 1.5])
  // With a gap before the third key, widening uses the gap up before pushing.
  await set(2, 'x', Number(await page.inputValue('[data-test=field-x]')) + 1)
  expect(await lefts()).toEqual([0, 0.5, 2.5])
  await set(0, 'w', 1.5)
  expect(await lefts()).toEqual([0, 1.5, 2.5])
  await set(0, 'w', 2)
  expect(await lefts()).toEqual([0, 2, 3])
  // After that push all three touch, so narrowing pulls both back.
  await set(0, 'w', 1)
  expect(await lefts()).toEqual([0, 1, 2])

  await select(0)
  await canvas.locator('[data-pos="1"]').dispatchEvent('pointerdown', { shiftKey: true })
  await page.dispatchEvent('body', 'pointerup')
  await expect(canvas.locator('.key.selected')).toHaveCount(2)
  // Dragging one of them moves both.
  const before = [await boxOf(0), await boxOf(1), await boxOf(2)]
  await page.mouse.move(before[0].x + 10, before[0].y + 10)
  await page.mouse.down()
  await page.mouse.move(before[0].x + 10, before[0].y + 10 + before[2].height * 3, { steps: 5 })
  await page.mouse.up()
  const after = [await boxOf(0), await boxOf(1), await boxOf(2)]
  expect(after[2].y - after[0].y).toBeLessThan(-10)
  expect(Math.abs(after[0].y - after[1].y)).toBeLessThan(2)
  // A click on the background clears the selection.
  const frame = (await canvas.boundingBox())!
  await page.mouse.click(frame.x + 6, frame.y + 6)
  await expect(canvas.locator('.key.selected')).toHaveCount(0)
})

test('split keyboard: the cable between the halves is found without asking', async ({ page }) => {
  await openDemo(page, 'split')
  await expect(page.locator('[data-test=cable-searching]')).toBeVisible()
  await expect(page.locator('[data-test=next]')).toBeDisabled()
  await expect(page.locator('[data-test=cable-found]')).toContainText('sends on GP0 and receives on GP1', { timeout: 15_000 })
  // The pin list is there for those who want it, with the found pair chosen.
  await page.click('[data-test=cable-by-hand]')
  await expect(page.locator('[data-test=link] option:checked')).toContainText('sends on GP0, receives on GP1')
  // One piece needs no cable.
  await page.check('[data-test=shape-single]')
  await expect(page.locator('[data-test=cable-found]')).toHaveCount(0)
  await expect(page.locator('[data-test=next]')).toBeEnabled()
})

test('split setup guides through both halves and waits for the second one', async ({ page }) => {
  // One board alone, declared to be half of a split.
  await openDemo(page, 'macropad')
  await openShape(page)
  await page.check('[data-test=shape-central]')
  const guide = page.locator('[data-test=split-guide]')
  await expect(guide.locator('li').nth(0)).toHaveClass(/done/)
  await expect(guide.locator('[data-test=other-waiting]')).toBeVisible()
  await expect(guide.locator('[data-test=cable-need-both]')).toBeVisible()
  await expect(page.locator('[data-test=next]')).toBeDisabled()
  // Knowing the pins is a way on without the second half.
  await page.click('[data-test=cable-by-hand]')
  await expect(page.locator('[data-test=next]')).toBeEnabled()

  // With both halves open every step ticks off by itself.
  await page.goto('/')
  await openDemo(page, 'split')
  await expect(page.locator('[data-test=guide-other]')).toHaveClass(/done/)
  await expect(page.locator('[data-test=guide-other]')).toContainText('Right half')
  await expect(page.locator('[data-test=guide-cable]')).toHaveClass(/done/, { timeout: 15_000 })
  await expect(page.locator('[data-test=next]')).toBeEnabled()

  // The home screen lists what is plugged in.
  await page.click('.rail .logo')
  await expect(page.locator('[data-test=found-boards] .found-row')).toHaveCount(2)
  await expect(page.locator('[data-test=found-boards]')).toContainText('new, not set up yet')
})

test('lights: switched on, placed one by one and given an effect per group', async ({ page }) => {
  await openDemo(page, 'knobpad')
  await runWizard(page, { knob: true })
  const bulbs = page.locator('[data-test=strip] .bulb')
  await expect(bulbs).toHaveCount(6)
  await expect(bulbs.first()).toHaveAttribute('data-color', '#000000')

  await page.click('[data-test=nav-lighting]')
  await page.selectOption('[data-test=light-pin]', { label: 'GP16' })
  await page.fill('[data-test=light-count]', '6')
  await page.click('[data-test=light-on]')
  // The board restarts with the pin in use and shows the starting rainbow.
  await expect(bulbs.first()).not.toHaveAttribute('data-color', '#000000')
  await expect(page.locator('[data-test=light-board] [data-light]')).toHaveCount(6)
  // To begin with they are underglow, spread around the keyboard.
  await expect(page.locator('[data-test=mode-underglow]')).toHaveClass(/on/)
  await expect(page.locator('[data-test=zone-underglow]')).toContainText('6')
  await expect(page.locator('[data-test=zone-keys]')).toHaveCount(0)

  // Per key: every light is under a key, and placing starts by itself.
  await page.click('[data-test=mode-keys]')
  await expect(page.locator('[data-test=placing]')).toContainText('Light 1 of 6')
  await expect(page.locator('[data-test=zone-keys]')).toContainText('6')
  await expect(page.locator('[data-test=zone-underglow]')).toHaveCount(0)

  // Both: the first four under the four keys, the last two as underglow.
  await page.click('[data-test=mode-both]')
  await expect(page.locator('[data-test=placing]')).toContainText('Light 1 of 6')
  await expect(bulbs.nth(1)).toHaveAttribute('data-color', '#000000')
  await expect(bulbs.first()).not.toHaveAttribute('data-color', '#000000')
  const key = (pos: number) => page.locator(`[data-test=light-board] [data-pos="${pos}"]`).first()
  for (let pos = 0; pos < 4; pos++) {
    await key(pos).dispatchEvent('pointerdown')
    await expect(page.locator('[data-test=placing]')).toContainText(`Light ${pos + 2} of 6`)
  }
  await page.check('[data-test=place-under]')
  await key(0).dispatchEvent('pointerdown')
  await key(3).dispatchEvent('pointerdown')
  await expect(page.locator('[data-test=placing]')).toHaveCount(0)
  await expect(page.locator('[data-test=zone-keys]')).toContainText('4')
  await expect(page.locator('[data-test=zone-underglow]')).toContainText('2')

  // Keys light up on a press only; the underglow is off.
  await page.selectOption('[data-test=effect-keys]', 'Reactive')
  await page.selectOption('[data-test=effect-underglow]', 'Off')
  await expect(bulbs.nth(2)).toHaveAttribute('data-color', '#000000')
  await virtualKeys(page).nth(2).dispatchEvent('pointerdown')
  await expect(bulbs.nth(2)).not.toHaveAttribute('data-color', '#000000')
  await expect(bulbs.nth(1)).toHaveAttribute('data-color', '#000000')
  await expect(bulbs.nth(5)).toHaveAttribute('data-color', '#000000')
  await page.dispatchEvent('body', 'pointerup')

  // It all survives a save and restart of the board.
  await page.selectOption('[data-test=effect-underglow]', 'Solid')
  await page.click('[data-test=save]')
  await expect(page.locator('[data-test=save]')).toHaveText('Saved')
  await expect(bulbs.nth(5)).not.toHaveAttribute('data-color', '#000000')
})

test('split keyboards are marked experimental in the setup', async ({ page }) => {
  await openDemo(page, 'split')
  await expect(page.locator('[data-test=experimental]')).toBeVisible()
})

test('keymap: drag from the picker, swap keys, colour a layer, see all layers', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  const key = (pos: number) => page.locator(`[data-test=keymap-board] [data-pos="${pos}"]`)

  // The picker shows an ordinary keyboard; Q is dragged onto the third key.
  await page.dragAndDrop('.picker [data-usage="20"]', '[data-test=keymap-board] [data-pos="2"]')
  await expect(key(2)).toHaveClass(/selected/)
  await expect(page.locator('[data-test=action-description]')).toHaveText('Q')
  await tap(page, 2)
  await expect(typed(page)).toHaveText('q')

  // Dragging the first key onto the second swaps them.
  const from = (await key(0).boundingBox())!
  const to = (await key(1).boundingBox())!
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2)
  await page.mouse.down()
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 6 })
  await page.mouse.up()
  await expect(key(0)).toContainText('B')
  await expect(key(1)).toContainText('A')
  // A plain click swaps nothing.
  await key(0).click()
  await expect(key(0)).toContainText('B')

  // A layer key wears the colour of the layer it leads to.
  await page.click('[data-test=add-layer]')
  await expect(page.locator('[data-test=layer-1]')).toHaveClass(/on/)
  await page.click('[data-test=layer-1]')
  await page.click('[data-test=color-4]')
  await expect(page.locator('[data-test=layer-colors]')).toHaveCount(0)
  await page.click('[data-test=layer-0]')
  await page.dragAndDrop('[data-test=layer-hold-1]', '[data-test=keymap-board] [data-pos="5"]')
  await expect(key(5)).toHaveClass(/tinted/)
  await expect(key(5)).toHaveAttribute('style', /#ef4444/)

  // The colour is kept by the keyboard.
  await page.click('[data-test=save]')
  await expect(page.locator('[data-test=save]')).toHaveText('Saved')

  // All layers as one picture, in both arrangements.
  await page.click('[data-test=all-layers]')
  const poster = page.locator('[data-test=poster] svg')
  await expect(poster).toContainText('Layer 0 · base')
  await expect(poster).toContainText('Layer 1')
  await expect(poster.locator('circle[fill="#ef4444"]')).toHaveCount(1)
  await page.click('[data-test=overview-stacked]')
  await expect(poster).toContainText('Layer 1')
  const download = page.waitForEvent('download')
  await page.click('[data-test=save-png]')
  expect((await download).suggestedFilename()).toMatch(/-keymap\.png$/)
})
