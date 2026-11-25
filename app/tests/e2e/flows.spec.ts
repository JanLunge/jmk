import { expect, test, type Page } from '@playwright/test'
import { assign, chord, giveLetters, hold, openDemo, openShape, pickDemo, plugInNew, release, runSplitWizard, runWizard, saved, showBench, skipArranging, tap, typed, virtualKeys } from './helpers'

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

  await saved(page)
  // A firmware update restarts the keyboard; saved settings must come back.
  await page.click('[data-test=update-firmware]')
  await expect(page.getByText('update mode')).toBeVisible()
  await expect(page.locator('[data-test=update-firmware]')).toBeVisible({ timeout: 10_000 })
  await tap(page, 0)
  await expect(typed(page)).toHaveText('afqq')
})

test('a keyboard file restores a board that is set up again', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  await page.fill('[data-test=rename]', 'Pad')
  await page.keyboard.press('Enter')
  await assign(page, 0, 0x14) // Q
  await page.click('[data-test=nav-share]')
  const [download] = await Promise.all([page.waitForEvent('download'), page.click('[data-test=export]')])
  const file = await download.path()

  // A new board, as after flashing anew: its setup takes the file instead of key presses.
  await page.goto('/')
  await openDemo(page, 'macropad')
  await expect(page.locator('[data-test=found]')).toBeVisible()
  await page.locator('[data-test=restore-file]').setInputFiles(file)
  await expect(page.locator('[data-test=restore-dialog]')).toContainText('Restore “Pad”')
  await page.click('[data-test=restore-apply]')
  await expect(page.locator('[data-test=rename]')).toHaveValue('Pad')
  await tap(page, 0)
  await tap(page, 1)
  await expect(typed(page)).toHaveText('qb')
})

test('setup: row ends give the picture; a macro pad is not asked for letter rows', async ({ page }) => {
  await openDemo(page, 'macropad')
  await expect(page.locator('[data-test=found]')).toBeVisible()
  await page.waitForTimeout(750)
  // The bottom row of the bench first: the order pressed is the order drawn.
  for (const key of [3, 4, 5, 0, 1, 2]) await tap(page, key)
  await expect(page.locator('[data-test=setup-guide]')).not.toContainText('Letters')
  await page.click('[data-test=next]')

  // The last key of the first row, pressed on the keyboard; a key not pressed before does not count.
  await expect(page.locator('[data-test=rows-1] .line')).toHaveCount(1)
  await tap(page, 5)
  await expect(page.locator('[data-test=rows-1] .line')).toHaveCount(2)
  await expect(page.locator('[data-test=row-key-1-2]')).toHaveClass(/end/)
  // Taken back and set again with a click.
  await page.click('[data-test=row-key-1-2]')
  await expect(page.locator('[data-test=rows-1] .line')).toHaveCount(1)
  await page.click('[data-test=row-key-1-2]')

  // Six keys cannot hold the alphabet: the rows are the last step.
  await expect(page.locator('[data-test=next]')).toHaveText('Finish')
  await page.click('[data-test=next]')
  await expect(page.locator('[data-test=rename]')).toBeVisible()
  await expect(page.locator('[data-test=letters-board]')).toHaveCount(0)

  // The first key pressed is top left; the row pressed second sits below it.
  const places = await page.locator('[data-test=keymap-board] [data-pos]').evaluateAll((keys) =>
    keys
      .map((key) => ({ pos: Number(key.getAttribute('data-pos')), box: key.getBoundingClientRect() }))
      .sort((a, b) => Math.round(a.box.y - b.box.y) || a.box.x - b.box.x)
      .map((key) => key.pos),
  )
  expect(places).toEqual([0, 1, 2, 3, 4, 5])
})

test('setup: painted rows give the letters on a keyboard that can hold them', async ({ page }) => {
  await openDemo(page, 'forty')
  await expect(page.locator('[data-test=found]')).toBeVisible()
  await page.waitForTimeout(750)
  for (let key = 0; key < 40; key++) await tap(page, key)
  await expect(page.locator('[data-test=setup-guide]')).toContainText('Letters')
  await page.click('[data-test=next]')
  for (const end of [9, 19, 29]) await tap(page, end)
  await expect(page.locator('[data-test=rows-1] .line')).toHaveCount(4)
  await expect(page.locator('[data-test=next]')).toHaveText('Continue')
  await page.click('[data-test=next]')

  // Colemak-DH: a click on the first key of a row takes the keys to its right too.
  const board = (pos: number) => page.locator(`[data-test=letters-board] [data-pos="${pos}"]`)
  await page.click('[data-test=alphas-colemak-dh]')
  // Positions are numbered in the order the keys were pressed.
  await board(0).dispatchEvent('pointerdown')
  await page.dispatchEvent('body', 'pointerup')
  await expect(page.locator('[data-test=letter-row-0]')).toContainText('10 keys')
  await expect(page.locator('[data-test=letter-row-1]')).toHaveClass(/on/)
  await board(10).dispatchEvent('pointerdown')
  await page.dispatchEvent('body', 'pointerup')
  await expect(board(2)).toHaveText('F')
  await expect(page.locator('[data-test=next]')).toHaveText('Finish')
  await page.click('[data-test=next]')
  await expect(page.locator('[data-test=rename]')).toBeVisible()

  await tap(page, 0)
  await tap(page, 2)
  await tap(page, 10)
  await expect(typed(page)).toHaveText('qfa')
})

test('split setup: rows on both halves, letters across them', async ({ page }) => {
  await openDemo(page, 'split')
  await expect(page.locator('[data-test=found-other]')).toBeVisible({ timeout: 20_000 })
  await page.waitForTimeout(750)
  const count = await virtualKeys(page).count()
  for (let key = 0; key < count; key++) await tap(page, key)
  await page.click('[data-test=next]')
  // Five keys a row and two thumb keys, on either half.
  for (const key of [4, 9, 14, 21, 26, 31]) await tap(page, key)
  await expect(page.locator('[data-test=rows-1] .line')).toHaveCount(4)
  await expect(page.locator('[data-test=rows-2] .line')).toHaveCount(4)
  await page.click('[data-test=next]')

  // QWERTY: each row runs on from the left half into the right one.
  for (const first of [0, 5, 10]) {
    await page.locator(`[data-test=letters-board] [data-pos="${first}"]`).dispatchEvent('pointerdown')
    await page.dispatchEvent('body', 'pointerup')
  }
  await expect(page.locator('[data-test=letter-row-2]')).toContainText('10 keys')
  await page.click('[data-test=next]')
  await expect(page.locator('[data-test=split-status]')).toHaveAttribute('title', /types for both/, { timeout: 10_000 })
  for (const key of [0, 17, 9, 31, 15]) await tap(page, key)
  // The thumb key types nothing yet.
  await expect(typed(page)).toHaveText('qyg/')

  // The keyboard remembers the rows: another arrangement is picked, not painted again.
  await page.locator('[data-test=keymap-board] [data-pos="0"]').dispatchEvent('pointerdown')
  await page.check('[data-test=hold-tap]') // Q, held: Shift
  await page.click('[data-test=letters-open]')
  await expect(page.locator('[data-test=letters-dialog] [data-test=alphas-qwerty]')).toHaveClass(/on/)
  await expect(page.locator('[data-test=letters-dialog] [data-test=letter-row-1]')).toContainText('10 keys')
  await page.click('[data-test=letters-dialog] [data-test=alphas-dvorak]')
  await page.click('[data-test=letters-apply]')
  await expect(page.locator('[data-test=letters-dialog]')).toHaveCount(0)
  for (const key of [0, 17, 9, 31]) await tap(page, key)
  await expect(typed(page)).toHaveText("qyg/'fiz")
  // The key that is Shift when held is still that.
  await expect(page.locator('[data-test=keymap-board] [data-pos="0"]')).toContainText('Shift')
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

const splitStatus = (page: Page) => page.locator('[data-test=split-status]')

test('split keyboard: a keyboard file restores both halves', async ({ page }) => {
  await openDemo(page, 'split')
  await runSplitWizard(page)
  await assign(page, 0, 0x14) // left half, first key: Q
  // The right half's first key, "r" until now: Z.
  await page.locator('[data-test=keymap-board] [data-pos]', { hasText: /^R$/ }).dispatchEvent('pointerdown')
  await page.click('.picker [data-usage="29"]')
  await page.click('[data-test=nav-share]')
  const [download] = await Promise.all([page.waitForEvent('download'), page.click('[data-test=export]')])
  const file = await download.path()

  await page.goto('/')
  await openDemo(page, 'split')
  await expect(page.locator('[data-test=found-other]')).toBeVisible({ timeout: 20_000 })
  await page.locator('[data-test=restore-file]').setInputFiles(file)
  await expect(page.locator('[data-test=restore-dialog]')).toContainText('across two halves')
  await page.click('[data-test=restore-apply]')
  await expect(splitStatus(page)).toHaveAttribute('title', /types for both/, { timeout: 15_000 })
  await tap(page, 0)
  await tap(page, 17)
  await expect(typed(page)).toHaveText('qz')
})
const toast = (page: Page, text: string) => page.locator('.toast', { hasText: text })

test('split keyboard: set up together, either half types, the keymap follows through the cable', async ({ page }) => {
  await openDemo(page, 'split')
  await runSplitWizard(page)
  // Two halves, one keyboard: one entry at the side, one keymap.
  await expect(page.locator('[data-test=rail-keyboard]')).toHaveCount(1)
  await expect(page.locator('[data-test=keymap-board] .key')).toHaveCount(34)
  // Typed at a person's pace: the header redraws meanwhile and must leave the field alone.
  await page.click('[data-test=rename]')
  await page.keyboard.type('Sweep', { delay: 400 })
  await expect(page.locator('[data-test=rename]')).toHaveValue('Sweep')
  await page.keyboard.press('Enter')
  await expect(page.locator('[data-test=rail-keyboard]')).toHaveAttribute('title', 'Sweep')
  await page.click('[data-test=split-status]')
  await expect(page.locator('[data-test=split-details]')).toContainText(/revision \d+, same on both/)
  await page.click('[data-test=split-details]')
  await expect(splitStatus(page)).toHaveAttribute('title', /the left half types for both/)

  // Both halves are plugged into the computer: only one types, for both.
  // Left key 0 is "a"; the right half's first key continues the alphabet after 17 left keys.
  await tap(page, 0)
  await tap(page, 17)
  await expect(typed(page)).toHaveText('ar')

  // With only the right half left on USB, it types for both, and the keyboard stays open.
  await page.click('[data-test=usb-0]')
  await expect(splitStatus(page)).toHaveAttribute('title', /right half on USB · left half through the cable/, { timeout: 8000 })
  await tap(page, 0)
  await tap(page, 17)
  await expect(typed(page)).toHaveText('arar')

  // A change saved there reaches the left half through the cable.
  const before = Number(await splitStatus(page).getAttribute('data-revision'))
  await assign(page, 1, 29) // Z
  await saved(page)
  await expect(splitStatus(page)).toHaveAttribute('data-revision', String(before + 1))

  // Without the cable each half is a keyboard of its own. The left half
  // lost its power in between and still has the new key.
  await page.click('[data-test=cable]')
  await page.click('[data-test=usb-0]')
  await expect(splitStatus(page)).toHaveAttribute('title', /halves apart/, { timeout: 8000 })
  await tap(page, 1)
  await tap(page, 18)
  await expect(typed(page)).toHaveText('ararzs')
})

test('split keyboard: a half that was away catches up; two changed halves ask which to keep', async ({ page }) => {
  await openDemo(page, 'split')
  await runSplitWizard(page)

  // The right half is put away; the keymap changes without it.
  await page.click('[data-test=cable]')
  await page.click('[data-test=usb-1]')
  await expect(splitStatus(page)).toHaveAttribute('title', /the right half is away/, { timeout: 8000 })
  await assign(page, 1, 29) // Z
  await saved(page)
  // Joined again by the cable alone, it fetches the keymap without being asked.
  await page.click('[data-test=cable]')
  await expect(toast(page, 'The right half caught up')).toBeVisible({ timeout: 10_000 })

  // Now each half is changed while the other is away.
  await page.click('[data-test=cable]')
  await assign(page, 2, 27) // X on the left
  await saved(page)
  await page.click('[data-test=usb-0]')
  await page.click('[data-test=usb-1]')
  await expect(splitStatus(page)).toHaveAttribute('title', /the left half is away/, { timeout: 8000 })
  await assign(page, 2, 28) // Y on the right
  await saved(page)
  // Both plugged in: nothing is overwritten until the user says which one counts.
  await page.click('[data-test=usb-0]')
  await expect(page.locator('[data-test=halves-conflict]')).toBeVisible({ timeout: 8000 })
  await tap(page, 2)
  await tap(page, 2)
  await expect(typed(page)).toHaveText('xx')
  await page.click('[data-test=keep-this]')
  await expect(toast(page, 'Both halves hold the same keymap now')).toBeVisible({ timeout: 10_000 })
  await expect(page.locator('[data-test=halves-conflict]')).toHaveCount(0)
  await tap(page, 2)
  await expect(typed(page)).toHaveText('xxy')
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
  // Kept at once, and the picture moves back to start at the left edge.
  await expect(page.locator('[data-test=field-x]')).toHaveValue('0')
  await page.fill('[data-test=field-w]', '2')
  await page.locator('[data-test=field-w]').blur()
  await page.click('[data-test=nav-keymap]')
  await expect(page.locator('[data-test=keymap-board] .key')).toHaveCount(6)
  const widths = await page.locator('[data-test=keymap-board] .key .base').evaluateAll((rects) => rects.map((r) => r.getAttribute('width')))
  expect(widths.filter((w) => w === '194')).toHaveLength(1)

  await page.click('[data-test=nav-layout]')
  await page.click('[data-test=import-kle]')
  await page.fill('[data-test=kle-text]', '["A","B","C","D","E","F"]')
  await page.click('[data-test=kle-apply]')
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
  await page.click('[data-test=macro-by-hand]')
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

test('a macro is named and recorded right in the keymap', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  const key = (pos: number) => page.locator(`[data-test=keymap-board] [data-pos="${pos}"]`)
  const steps = page.locator('[data-test=macro-steps] li')
  const titles = () => steps.evaluateAll((items) => items.map((item) => item.getAttribute('title')))

  // Macro 3 goes on the first key; its editor opens beside the list.
  await key(0).dispatchEvent('pointerdown')
  await page.click('[data-test=tab-macros]')
  await page.click('[data-test=macro-2]')
  await expect(page.locator('[data-test=macro-name]')).toHaveAttribute('placeholder', 'Macro 3')

  // Named at a person's pace, while the keyboard keeps reporting.
  await page.locator('[data-test=macro-name]').pressSequentially('Greet', { delay: 300 })
  await page.keyboard.press('Enter')
  await expect(page.locator('[data-test=macro-2]')).toContainText('Greet')
  await expect(key(0)).toContainText('Greet')
  await expect(page.locator('[data-test=action-description]')).toHaveText('Macro: Greet')

  // Shift+H, then I, played slowly. Nothing of it reaches the page while recording.
  await page.click('[data-test=macro-record]')
  await page.keyboard.down('ShiftLeft')
  await page.waitForTimeout(150)
  await page.keyboard.press('KeyH', { delay: 60 })
  await page.waitForTimeout(100)
  await page.keyboard.up('ShiftLeft')
  await page.waitForTimeout(300)
  await page.keyboard.press('KeyI', { delay: 60 })
  await expect(page.locator('[data-test=macro-heard] .chip')).toHaveCount(3)
  await page.click('[data-test=macro-stop]')
  // The keys went into the recording only: the selected key still holds the macro.
  await expect(key(0)).toContainText('Greet')

  // Just the keys: four steps and no waiting.
  await expect.poll(titles).toEqual(['Hold Left Shift', 'Tap H', 'Release Left Shift', 'Tap I'])
  // The same recording as it was played, without recording again.
  await page.click('[data-test=timing-played]')
  await expect(steps).toHaveCount(11)
  await expect(page.locator('[data-test=macro-steps] li.wait')).toHaveCount(5)
  // And back, with an even pause between the keys.
  await page.click('[data-test=timing-order]')
  await page.selectOption('[data-test=timing-pause]', '30')
  await expect.poll(titles).toEqual(['Hold Left Shift', 'Wait 30 ms', 'Tap H', 'Release Left Shift', 'Wait 30 ms', 'Tap I'])

  // The keyboard stops a macro that is replaced while it plays, so let the change arrive first.
  await page.waitForTimeout(400)
  await tap(page, 0)
  await expect(typed(page)).toHaveText('Hi')

  // The same macro, with its name, under Behaviours.
  await page.click('[data-test=nav-behaviours]')
  await page.click('[data-test=section-macros]')
  await expect(page.locator('[data-test=macro-slot-2]')).toContainText('Greet')
  await page.click('[data-test=macro-slot-2]')
  await expect(page.locator('[data-test=macro-name]')).toHaveValue('Greet')
  await expect(steps).toHaveCount(6)
})

test('keymap: several keys are selected and changed together', async ({ page }) => {
  await openDemo(page, 'macropad')
  await runWizard(page)
  const key = (pos: number) => page.locator(`[data-test=keymap-board] [data-pos="${pos}"]`)
  const legends = () => page.locator('[data-test=keymap-board] [data-pos]').allTextContents()

  // Shift-click adds keys; what is picked goes on all of them.
  await key(0).dispatchEvent('pointerdown')
  await key(2).dispatchEvent('pointerdown', { shiftKey: true })
  await key(4).dispatchEvent('pointerdown', { shiftKey: true })
  await expect(page.locator('[data-test=picker-target]')).toHaveText('3 keys')
  await page.click('.picker [data-usage="30"]') // 1
  await expect.poll(async () => (await legends()).filter((text) => text.trim() === '1').length).toBe(3)
  await expect(page.locator('[data-test=action-description]')).toHaveText('1')

  // A plain click is one key again.
  await key(1).dispatchEvent('pointerdown')
  await expect(page.locator('[data-test=picker-target]')).toHaveText('This key')

  // Dragging across the board selects what the rectangle touches: here every key.
  const box = (await page.locator('[data-test=keymap-board]').boundingBox())!
  await page.mouse.move(box.x + 2, box.y + 2)
  await page.mouse.down()
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 4 })
  await page.mouse.move(box.x + box.width - 2, box.y + box.height - 2, { steps: 4 })
  await page.mouse.up()
  const count = await page.locator('[data-test=keymap-board] [data-pos]').count()
  await expect(page.locator('[data-test=picker-target]')).toHaveText(`${count} keys`)
  await page.click('[data-test=tab-more]')
  await page.click('[data-test=none]')
  await expect(page.locator('[data-test=action-description]')).toHaveText('Nothing')
  await tap(page, 0)
  await tap(page, 3)
  await expect(typed(page)).toHaveText('')
})

test('a duplex matrix and keys on pins of their own are found on one board', async ({ page }) => {
  await openDemo(page, 'duplex')
  await runWizard(page)
  await expect(page.locator('[data-test=key-count]')).toHaveText('15 keys')
  // Every switch has a position of its own: fifteen taps, fifteen different letters.
  for (let key = 0; key < 15; key++) await tap(page, key)
  const letters = (await typed(page).innerText()).replace(/\s/g, '')
  expect(new Set(letters).size).toBe(15)

  await page.click('[data-test=nav-hardware]')
  const summary = page.locator('[data-test=wiring-summary]')
  await expect(summary).toContainText('Duplex matrix: rows GP2, GP3; columns GP4, GP5, GP6')
  await expect(summary).toContainText('2 keys, each on its own pin (GP10, GP11)')
  await expect(summary).toContainText('1 key on its own pin to supply voltage (GP12)')

  // The wiring is kept through a restart.
  await saved(page)
  await page.click('[data-test=update-firmware]')
  await expect(page.getByText('update mode')).toBeVisible()
  await expect(page.locator('[data-test=update-firmware]')).toBeVisible({ timeout: 10_000 })
  await expect(page.locator('[data-test=wiring-summary]')).toContainText('1 key on its own pin to supply voltage (GP12)')
  await tap(page, 14)
  await expect(typed(page)).not.toBeEmpty()
})

test('forty keys: a full hand-wired matrix is found', async ({ page }) => {
  test.setTimeout(90_000)
  await openDemo(page, 'forty')
  await runWizard(page)
  await page.click('[data-test=nav-hardware]')
  await expect(page.locator('[data-test=wiring-summary]')).toContainText('rows GP2, GP3, GP4, GP5; columns GP6')
  await expect(page.locator('[data-test=key-count]')).toHaveText('40 keys')
})

test('changes save themselves and can be undone; an empty keyboard offers its letters', async ({ page }) => {
  test.setTimeout(90_000)
  await openDemo(page, 'forty')
  await runWizard(page, { empty: true })
  const first = page.locator('[data-test=keymap-board] [data-pos="0"]')
  const undo = page.locator('[data-test=undo]')
  const redo = page.locator('[data-test=redo]')
  await expect(undo).toBeDisabled()

  // The key picker folds down to one bar, and searching for a key opens it again.
  await page.click('[data-test=picker-fold]')
  await expect(page.locator('[data-test=tab-keys]')).toBeHidden()
  await expect(page.locator('[data-test=action-description]')).toBeVisible()
  await page.fill('[data-test=search]', 'esc')
  await expect(page.locator('[data-test=picker] .key-btn').first()).toBeVisible()
  await page.fill('[data-test=search]', '')
  await expect(page.locator('[data-test=tab-keys]')).toBeVisible()

  // Nothing types yet, and the quick way to the letters is pointed out.
  await expect(page.locator('[data-test=letters-nudge]')).toBeVisible()
  await page.click('[data-test=letters-nudge-open]')
  await expect(page.locator('[data-test=letters-dialog]')).toBeVisible()
  await page.keyboard.press('Escape')
  await page.locator('[data-test=letters-dialog] .btn.ghost').click()

  await assign(page, 0, 0x14) // Q
  await expect(first).toContainText('Q')
  await expect(page.locator('[data-test=letters-nudge]')).toHaveCount(0)
  await saved(page)
  await tap(page, 0)
  await expect(typed(page)).toHaveText('q')

  // Saved already, and still taken back: on the keyboard too.
  await undo.click()
  await expect(first).not.toContainText('Q')
  await saved(page)
  await tap(page, 0)
  await expect(typed(page)).toHaveText('q')
  await expect(undo).toBeDisabled()
  await redo.click()
  await expect(first).toContainText('Q')

  // The picture of the keys is kept as it is changed, and undone the same way.
  await page.click('[data-test=nav-layout]')
  await page.locator('[data-test=layout-canvas] [data-pos="0"]').dispatchEvent('pointerdown')
  await page.dispatchEvent('body', 'pointerup')
  await page.fill('[data-test=field-w]', '2')
  await page.locator('[data-test=field-w]').blur()
  await saved(page)
  await page.click('[data-test=nav-keymap]')
  const width = () => page.locator('[data-test=keymap-board] [data-pos="0"] .base').getAttribute('width')
  const wide = await width()
  await page.keyboard.press('ControlOrMeta+z')
  await expect.poll(width).not.toBe(wide)
  await expect(redo).toBeEnabled()
  await page.keyboard.press('ControlOrMeta+Shift+z')
  await expect.poll(width).toBe(wide)
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
  await skipArranging(page)
  await expect(page.locator('[data-test=key-count]')).toHaveText('5 keys')

  await page.click('[data-test=nav-hardware]')
  await page.click('[data-test=add-key]')
  await expect(page.locator('[data-test=add-key-listening]')).toBeVisible()
  await page.waitForTimeout(750)
  await tap(page, 0) // already known
  await expect(page.locator('[data-test=add-key-problem]')).toContainText('already part')
  await tap(page, 5)
  await expect(page.locator('[data-test=key-count]')).toHaveText('6 keys')
  await expect(page.locator('[data-test=add-key]')).toBeVisible()
  // Like every key of a new keyboard it starts empty.
  await page.click('[data-test=nav-keymap]')
  await giveLetters(page)
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

test('split keyboard: new boards are found, flashed and joined without questions', async ({ page }) => {
  await page.goto('/')
  await pickDemo(page, 'split')
  // Two new controllers on the desk. Plugged in, they are offered firmware together.
  await page.click('[data-test=usb-0]')
  await expect(page.locator('[data-test=found-boot]')).toContainText('A board without firmware is connected.')
  await page.click('[data-test=usb-1]')
  await expect(page.locator('[data-test=found-boot]')).toContainText('Two boards without firmware are connected.')
  await expect(page.locator('[data-test=setup-guide]')).toContainText('Find the cable')
  await page.click('[data-test=install-found]')
  // The cable between them is looked for without asking.
  await expect(page.locator('[data-test=split-searching]')).toBeVisible()
  await expect(page.locator('[data-test=next]')).toBeDisabled()
  // Found, it is shown for a moment before the keys take over.
  await expect(page.locator('[data-test=split-searching] [data-test=picture-cable-found]')).toHaveAttribute('data-found', 'true', { timeout: 20_000 })
  const heading = page.locator('[data-test=keys-heading]')
  await expect(heading).toHaveText('Press any key under your left hand.', { timeout: 20_000 })
  await expect(page.locator('[data-test=rail-keyboard]')).toHaveCount(1)

  // The half pressed first is the left one, which can be put right and taken back.
  await page.waitForTimeout(750)
  await tap(page, 17)
  await expect(heading).toHaveText('Press every key once.')
  await expect(page.locator('[data-test=found-1]')).toContainText('1 keys')
  await page.click('[data-test=swap-sides]')
  await expect(page.locator('[data-test=found-1]')).toContainText('no key pressed')
  await page.click('[data-test=undo-key]')
  await expect(heading).toHaveText('Press any key under your left hand.')

  // Nothing goes on before each half has had a key pressed.
  await tap(page, 0)
  await expect(page.locator('[data-test=next]')).toBeDisabled()
  await tap(page, 17)
  await expect(page.locator('[data-test=next]')).toBeEnabled()
  // A half unplugged in the middle keeps its keys and carries on when it is back.
  await page.click('[data-test=usb-1]')
  await expect(page.locator('[data-test=lonely-half]')).toContainText('Its 1 key is kept')
  await expect(page.locator('[data-test=next]')).toBeDisabled()
  await page.click('[data-test=usb-1]')
  await expect(page.locator('[data-test=lonely-half]')).toHaveCount(0)
  await page.waitForTimeout(900)
  await tap(page, 18)
  await expect(page.locator('[data-test=count]')).toContainText('3 keys')

  await openShape(page)
  await expect(page.locator('[data-test=cable-found]')).toContainText('sends on GP0 and receives on GP1')
  // One board can still be set up by itself.
  await page.click('[data-test=set-up-alone]')
  await page.click('[data-test=next]')
  await expect(page.locator('h1')).toHaveText('Press every key once.')
})

test('split setup waits for the second half', async ({ page }) => {
  // One board alone: the page says what to do to get the other half in.
  await openDemo(page, 'macropad')
  await openShape(page)
  const guide = page.locator('[data-test=split-guide]')
  await expect(guide.locator('li').nth(0)).toHaveClass(/done/)
  await expect(guide.locator('[data-test=other-waiting]')).toBeVisible()
  await expect(guide.locator('[data-test=cable-need-both]')).toBeVisible()
  // Going back, the board is set up on its own as before.
  await page.click('[data-test=back]')
  await expect(page.locator('h1')).toHaveText('Press every key once.')

  // With both halves open every step ticks off by itself.
  await page.goto('/')
  await openDemo(page, 'split')
  await expect(page.locator('[data-test=keys-heading]')).toBeVisible({ timeout: 20_000 })
  await openShape(page)
  await expect(page.locator('[data-test=guide-other]')).toHaveClass(/done/)
  await expect(page.locator('[data-test=guide-other]')).toContainText('Right half')
  await expect(page.locator('[data-test=guide-cable]')).toHaveClass(/done/)

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
  // The board takes the pin into use at once and shows the starting rainbow.
  await expect(bulbs.first()).not.toHaveAttribute('data-color', '#000000')
  await expect(page.locator('[data-test=light-board] [data-light]')).toHaveCount(6)
  // To begin with they are underglow, spread around the keyboard.
  await expect(page.locator('[data-test=chain-summary]')).toHaveText('6 lights on GP16 · underglow')
  // The wiring is folded away until it is needed.
  await page.click('[data-test=chain-toggle]')
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
  await page.click('[data-test=effect-keys-Reactive]')
  await page.click('[data-test=effect-underglow-Off]')
  await expect(bulbs.nth(2)).toHaveAttribute('data-color', '#000000')
  await showBench(page)
  await virtualKeys(page).nth(2).dispatchEvent('pointerdown')
  await expect(bulbs.nth(2)).not.toHaveAttribute('data-color', '#000000')
  await expect(bulbs.nth(1)).toHaveAttribute('data-color', '#000000')
  await expect(bulbs.nth(5)).toHaveAttribute('data-color', '#000000')
  await page.dispatchEvent('body', 'pointerup')

  // It all survives a save and restart of the board.
  await page.click('[data-test=effect-underglow-Solid]')
  await saved(page)
  await expect(bulbs.nth(5)).not.toHaveAttribute('data-color', '#000000')
})

test('split keyboard: finding the keys again covers both halves', async ({ page }) => {
  await openDemo(page, 'split')
  await runSplitWizard(page)
  await page.click('[data-test=nav-hardware]')
  await expect(page.locator('[data-test=add-key-split]')).toBeVisible()
  await page.click('[data-test=rerun]')
  await page.click('[data-test=rerun-confirm]')
  // The cable is known already, so there is no search this time.
  await expect(page.locator('[data-test=keys-heading]')).toBeVisible()
  await runSplitWizard(page)
  await expect(page.locator('[data-test=keymap-board] .key')).toHaveCount(34)
})

test('split keyboards are marked experimental in the setup', async ({ page }) => {
  await openDemo(page, 'split')
  await expect(page.locator('[data-test=experimental]')).toBeVisible({ timeout: 20_000 })
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
  await page.click('[data-test=tab-layers]')
  await page.dragAndDrop('[data-test=layer-hold-1]', '[data-test=keymap-board] [data-pos="5"]')
  await expect(key(5)).toHaveClass(/tinted/)
  await expect(key(5)).toHaveAttribute('style', /#ef4444/)

  // The colour is kept by the keyboard.
  await saved(page)

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

test('setup: one keyboard in one piece is guided through, and its controller works itself out', async ({ page }) => {
  await openDemo(page, 'macropad')
  // No second board, so no cable to find.
  const guide = page.locator('.wizard [data-test=setup-guide]')
  await expect(guide).toContainText('Find the keys')
  await expect(guide).not.toContainText('Find the cable')
  await expect(guide.locator('[aria-current=step]')).toHaveText('Find the keys')
  // An RP2040 board does not say which shape it has; the pins in use do.
  const controller = page.locator('[data-test=show-pins]')
  await expect(controller).toContainText('RP2040 controller')
  await expect(controller).not.toContainText('long')
  await page.waitForTimeout(750)
  await tap(page, 0)
  await expect(controller).toContainText('RP2040 controller, long')
  // The small picture opens the whole pinout.
  await controller.click()
  await expect(page.locator('[data-test=pins-diagram]')).toBeVisible()
  await page.click('[data-test=close-pins]')
  await page.click('[data-test=undo-key]')
  await expect(page.locator('[data-test=count]')).toContainText('0 keys')

  await runWizard(page)
  // Straight into the editor: no page in between.
  await expect(page.locator('[data-test=rename]')).toHaveValue('Macro pad')
})

test('two virtual keyboards stay apart and can be switched between', async ({ page }) => {
  test.setTimeout(120_000)
  // A macro pad in the middle of its setup, then a split is put on the desk as well.
  await openDemo(page, 'macropad')
  await page.click('.rail .logo')
  await pickDemo(page, 'split')
  await plugInNew(page)
  await expect(page.locator('[data-test=found-other]')).toBeVisible({ timeout: 20_000 })
  const keyboards = page.locator('[data-test=rail-keyboard]')
  await expect(keyboards).toHaveCount(2)

  // The macro pad's setup knows nothing of the split's boards.
  await keyboards.first().click()
  await expect(page.locator('[data-test=virtual-board]')).toContainText('Macro pad')
  await expect(page.locator('.wizard')).toHaveCount(1)
  await expect(page.locator('.wizard [data-test=setup-guide]')).not.toContainText('Find the cable')
  await expect(page.locator('[data-test=arrival]')).toHaveCount(0)
  await expect(page.locator('[data-test=virtual-board]')).toContainText('Macro pad')
  await runWizard(page)
  await expect(page.locator('[data-test=rename]')).toHaveValue('Macro pad')

  // Back to the split, which is where it was left.
  await keyboards.nth(1).click()
  await expect(page.locator('[data-test=keys-heading]')).toHaveText('Press any key under your left hand.')
  await expect(page.locator('[data-test=virtual-board]')).toContainText('Split keyboard')
  await runSplitWizard(page)
  await keyboards.first().click()
  await expect(page.locator('[data-test=rename]')).toHaveValue('Macro pad')
})

test('setup: a key press is acknowledged with a tone, which can be turned off', async ({ page }) => {
  // Count the tones instead of listening to them.
  await page.addInitScript(() => {
    const tones: number[] = ((window as unknown as { tones: number[] }).tones = [])
    // A pitch is set in hertz, a volume well below one.
    const set = AudioParam.prototype.setValueAtTime
    AudioParam.prototype.setValueAtTime = function (value: number, when: number) {
      if (value > 20) tones.push(value)
      return set.call(this, value, when)
    }
  })
  const tones = () => page.evaluate(() => (window as unknown as { tones: number[] }).tones)
  await openDemo(page, 'macropad')
  await page.waitForTimeout(750)

  await tap(page, 0)
  await expect(page.locator('[data-test=count]')).toContainText('1 key')
  expect(await tones()).toHaveLength(1)

  // The same key again sounds lower: nothing new was found.
  await tap(page, 0)
  const [fresh, again] = await tones()
  expect(again).toBeLessThan(fresh)

  await page.click('[data-test=sound]')
  await tap(page, 1)
  await expect(page.locator('[data-test=count]')).toContainText('2 keys')
  expect(await tones()).toHaveLength(2)

  // Off stays off the next time the app opens.
  await page.reload()
  expect(await page.evaluate(() => localStorage.getItem('jmk.sound'))).toBe('off')
})
