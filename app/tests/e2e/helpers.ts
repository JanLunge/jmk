import { expect, type Page } from '@playwright/test'

export const virtualKeys = (page: Page) => page.locator('[data-test=virtual-board] .key')

/** Hold a key of the virtual keyboard in front for a moment. */
export async function tap(page: Page, key: number, holdMs = 110) {
  await virtualKeys(page).nth(key).dispatchEvent('pointerdown')
  await page.waitForTimeout(holdMs)
  await page.dispatchEvent('body', 'pointerup')
  await page.waitForTimeout(70)
}

export async function hold(page: Page, key: number) {
  await virtualKeys(page).nth(key).dispatchEvent('pointerdown', { shiftKey: true })
  await page.waitForTimeout(90)
}

export const release = hold

export const typed = (page: Page) => page.locator('[data-test=typed]')

export async function openDemo(page: Page, id: string) {
  await page.goto('/')
  await page.click(`[data-test=demo-${id}]`)
  await expect(page.locator('[data-test=next]')).toBeVisible()
}

/** The page asking whether the keyboard is one piece or two halves. */
export async function openShape(page: Page) {
  const single = page.locator('[data-test=shape-single]')
  const link = page.locator('[data-test=open-shape]')
  await expect(single.or(link)).toBeVisible()
  if (await link.isVisible()) await link.click()
  await expect(single).toBeVisible()
}

interface SetupOptions {
  shape?: 'single' | 'central' | 'peripheral'
  knob?: boolean
  /** Which button ends the wizard. */
  finish?: 'finish' | 'finish-partner'
}

/** Click through the setup wizard for the keyboard in front, pressing every key. */
export async function runWizard(page: Page, options: SetupOptions = {}) {
  // The setup starts at pressing keys; a split starts at its cable.
  if (options.shape) {
    await openShape(page)
    await page.check(`[data-test=shape-${options.shape}]`)
    await page.click('[data-test=next]')
  }
  await expect(page.locator('[data-test=found]')).toBeVisible()
  await page.waitForTimeout(750) // stuck-contact check at the start of probing
  const count = await virtualKeys(page).count()
  for (let key = 0; key < count; key++) await tap(page, key)
  await expect(page.locator('[data-test=count]')).toContainText(`${count} key`)
  if (options.knob) {
    await page.click('[data-test=find-knob]')
    for (let i = 0; i < 4; i++) {
      await page.click('[data-test=knob-cw-0]')
      await page.waitForTimeout(140)
    }
    await expect(page.locator('.knobs .chip')).toContainText('knob 1')
  }
  await page.click('[data-test=next]')
  await page.click(`[data-test=${options.finish ?? 'finish'}]`)
}

/** In the keymap tab: select a key and give it a plain key from the picker. */
export async function assign(page: Page, pos: number, usage: number) {
  await page.locator(`[data-test=keymap-board] [data-pos="${pos}"]`).dispatchEvent('pointerdown')
  await page.click(`.picker [data-usage="${usage}"]`)
}

/** Press several keys in the same instant and let them go again. */
export async function chord(page: Page, keys: number[]) {
  for (const key of keys) await virtualKeys(page).nth(key).dispatchEvent('pointerdown')
  await page.waitForTimeout(120)
  await page.dispatchEvent('body', 'pointerup')
  await page.waitForTimeout(70)
}
