import { expect, type Page } from '@playwright/test'

export const virtualKeys = (page: Page) => page.locator('[data-test=virtual-board] .key')

/** In the editor the virtual bench is folded to a strip; its keys are needed here. */
export async function showBench(page: Page) {
  if ((await virtualKeys(page).count()) === 0) await page.click('[data-test=bench-toggle]')
}

/** Hold a key of the virtual keyboard in front for a moment. */
export async function tap(page: Page, key: number, holdMs = 110) {
  await showBench(page)
  await virtualKeys(page).nth(key).dispatchEvent('pointerdown')
  await page.waitForTimeout(holdMs)
  await page.dispatchEvent('body', 'pointerup')
  await page.waitForTimeout(70)
}

export async function hold(page: Page, key: number) {
  await showBench(page)
  await virtualKeys(page).nth(key).dispatchEvent('pointerdown', { shiftKey: true })
  await page.waitForTimeout(90)
}

export const release = hold

export const typed = (page: Page) => page.locator('[data-test=typed]')

export async function openDemo(page: Page, id: string) {
  await page.goto('/')
  await pickDemo(page, id)
  await plugInNew(page)
  // The setup has opened; for a split it starts by finding the cable.
  await expect(page.locator('.wizard [data-test=setup-guide]')).toContainText('Find the keys')
  await expect(page.locator('[data-test=special], [data-test=split-searching]').first()).toBeVisible()
}

/** Virtual keyboards sit behind one button on the first screen. */
export async function pickDemo(page: Page, id: string) {
  await page.click('[data-test=demos]')
  await page.click(`[data-test=demo-${id}]`)
}

/** The setup's rarely needed things sit behind one menu. */
export async function special(page: Page, item: string) {
  await page.click('[data-test=special]')
  await page.click(`[data-test=${item}]`)
}

/** A virtual keyboard starts as new boards on the desk: plug them in and put the firmware on them. */
export async function plugInNew(page: Page) {
  await page.click('[data-test=plug-all]')
  await page.click('[data-test=install-found]')
}

/** The page about setting up two halves together. */
export async function openShape(page: Page) {
  await special(page, 'open-shape')
  await expect(page.locator('[data-test=split-guide]')).toBeVisible()
}

interface SetupOptions {
  knob?: boolean
}

/** Click through the setup wizard for the keyboard in front, pressing every key. */
export async function runWizard(page: Page, options: SetupOptions = {}) {
  await expect(page.locator('[data-test=found]')).toBeVisible()
  await page.waitForTimeout(750) // stuck-contact check at the start of probing
  const count = await virtualKeys(page).count()
  for (let key = 0; key < count; key++) await tap(page, key)
  await expect(page.locator('[data-test=count]')).toContainText(`${count} key`)
  if (options.knob) {
    await special(page, 'find-knob')
    for (let i = 0; i < 4; i++) {
      await page.click('[data-test=knob-cw-0]')
      await page.waitForTimeout(140)
    }
    await expect(page.locator('.knobs .chip')).toContainText('knob 1')
  }
  // The last key press saves the keyboard and opens the editor.
  await page.click('[data-test=next]')
  await expect(page.locator('[data-test=rename]')).toBeVisible()
}

/**
 * Set up the split demo: the app finds the cable between the two boards by
 * itself, then the keys of both halves are pressed on one page.
 */
export async function runSplitWizard(page: Page) {
  await expect(page.locator('[data-test=found-other]')).toBeVisible({ timeout: 20_000 })
  await page.waitForTimeout(750)
  // Both halves lie on the bench; the left one's keys come first.
  const count = await virtualKeys(page).count()
  for (let key = 0; key < count; key++) await tap(page, key)
  await expect(page.locator('[data-test=count]')).toContainText(`${count} keys`)
  await page.click('[data-test=next]')
  await expect(page.locator('[data-test=split-status]')).toHaveAttribute('title', /types for both/, { timeout: 10_000 })
}

/** In the keymap tab: select a key and give it a plain key from the picker. */
export async function assign(page: Page, pos: number, usage: number) {
  await page.locator(`[data-test=keymap-board] [data-pos="${pos}"]`).dispatchEvent('pointerdown')
  await page.click(`.picker [data-usage="${usage}"]`)
}

/** Press several keys in the same instant and let them go again. */
export async function chord(page: Page, keys: number[]) {
  await showBench(page)
  for (const key of keys) await virtualKeys(page).nth(key).dispatchEvent('pointerdown')
  await page.waitForTimeout(120)
  await page.dispatchEvent('body', 'pointerup')
  await page.waitForTimeout(70)
}
