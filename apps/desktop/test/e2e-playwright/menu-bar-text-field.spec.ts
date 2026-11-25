/**
 * Linux: with a text field focused, only a pointer click on a file-moving row of the page-drawn bar
 * runs it (task 63 step 9 of `docs/specs/windows-port-plan.md`, `routes/(main)/text-field-gate.ts`).
 * Every key stays refused, the menu's own keys too: a stray Alt, then File's letter, then Delete's
 * letter once opened File > Delete from a field (review of #1281), and with "Skip confirmation" on
 * that trashes with no prompt. Real X keys and clicks only (`helpers/x11-keys.ts`), every letter
 * read off the menu's own underlines, so no label is read.
 *
 * Regression anchor (#1464): the bar taking the keyboard re-ran an explorer focus effect that pulled
 * focus from the field into the file list, and the stray-Alt pick then trashed `file-b.txt`. A
 * pick's target is now where focus was when the menu opened, and the effect no longer re-runs.
 */
import os from 'node:os'
import { test, expect } from './fixtures.js'
import {
  dispatchMenuCommand,
  ensureAppReady,
  escapeOverlayUntilGone,
  moveCursorToFile,
  type PageLike,
} from './helpers.js'
import { clickRealAt, focusMainWindow, mainWindowId, pressRealKey } from './helpers/x11-keys.js'
import { waitBudget } from './wait-budget.js'

const TITLES = '[role="menubar"] > [role="menuitem"]'
const BAR_MENU = '[role="menu"][data-command-menu="bar"]'
const BUTTON_MENU = '[role="menu"][data-command-menu="context"]'
const MAIN_MENU_BUTTON = '.main-menu-button'
const DELETE_DIALOG = '[data-dialog-id="delete-confirmation"]'
const FIELD_ID = 'e2e-text-field-keys'

/** A text field the person is typing in, in the main window: the gate asks only that one has focus. */
async function focusField(page: PageLike): Promise<void> {
  await page.evaluate(`(function(){
      var field = document.getElementById(${JSON.stringify(FIELD_ID)}) || document.createElement('input');
      field.id = ${JSON.stringify(FIELD_ID)};
      field.value = 'typing';
      document.body.append(field);
      field.focus();
  })()`)
  await expect
    .poll(() => page.evaluate<boolean>(`document.activeElement?.id === ${JSON.stringify(FIELD_ID)}`), {
      timeout: waitBudget(3000),
    })
    .toBe(true)
}

/** What's up: the delete prompt, and how many bar or button menus are open. */
function state(page: PageLike): Promise<{ deleteDialog: boolean; menus: number }> {
  return page.evaluate<{ deleteDialog: boolean; menus: number }>(`({
      deleteDialog: document.querySelector(${JSON.stringify(DELETE_DIALOG)}) !== null,
      menus: document.querySelectorAll(${JSON.stringify(`${BAR_MENU}, ${BUTTON_MENU}`)}).length,
  })`)
}

/** The access letter a row of an open menu underlines, lowercased. */
async function rowLetter(page: PageLike, menu: string, menuId: string): Promise<string> {
  const letter = await page.evaluate<string | null>(`(function(){
      var row = document.querySelector(${JSON.stringify(`${menu} [data-menu-id="${menuId}"]`)});
      var key = row && row.querySelector('.command-menu-access-key');
      return key ? key.textContent.toLowerCase() : null;
  })()`)
  if (!letter) throw new Error(`no access letter on \`${menuId}\``)
  return letter
}

/** The File title's letter, off its `aria-keyshortcuts` (`Alt+F`). */
async function fileTitleLetter(page: PageLike): Promise<string> {
  const shortcut = await page.evaluate<string | null>(
    `document.querySelector(${JSON.stringify(TITLES)})?.getAttribute('aria-keyshortcuts') ?? null`,
  )
  expect(shortcut).toMatch(/^Alt\+.$/u)
  return (shortcut ?? '').slice(-1).toLowerCase()
}

/** Asserts no delete prompt opens: polls a moment, since a pick's relay is one round trip. */
async function expectNoDeletePrompt(page: PageLike): Promise<void> {
  await expect.poll(() => state(page), { timeout: waitBudget(1500) }).toMatchObject({ deleteDialog: false })
  expect((await state(page)).deleteDialog).toBe(false)
}

/** Lets go of any open menu and the bar, as Escape does. */
async function releaseMenus(page: PageLike): Promise<void> {
  for (let tries = 0; tries < 4 && (await state(page)).menus > 0; tries++) {
    focusMainWindow()
    pressRealKey('Escape')
  }
  focusMainWindow()
  pressRealKey('Escape')
}

async function cleanUp(page: PageLike): Promise<void> {
  if ((await state(page)).deleteDialog) await escapeOverlayUntilGone(page, DELETE_DIALOG)
  await releaseMenus(page)
  await page.evaluate(`document.getElementById(${JSON.stringify(FIELD_ID)})?.remove()`)
}

test.describe('The text-field gate on a bar the page draws (Linux)', () => {
  test.skip(os.platform() !== 'linux', 'The page draws the bar off macOS; the Linux lane drives it with real X keys')

  test.afterEach(async ({ tauriPage }) => {
    await cleanUp(tauriPage)
  })

  test('the bar’s own keys never run File > Delete from a text field, and a click on it does', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    expect(await moveCursorToFile(tauriPage, 'file-b.txt')).toBe(true)
    const fileLetter = await fileTitleLetter(tauriPage)

    // A stray lone Alt, then File's letter, then Delete's.
    await focusField(tauriPage)
    focusMainWindow()
    pressRealKey('alt')
    pressRealKey(fileLetter)
    await expect.poll(() => state(tauriPage), { timeout: waitBudget(3000) }).toMatchObject({ menus: 1 })
    pressRealKey(await rowLetter(tauriPage, BAR_MENU, 'file_delete'))
    await expectNoDeletePrompt(tauriPage)
    await releaseMenus(tauriPage)

    // Alt plus File's letter, then Delete's.
    await focusField(tauriPage)
    focusMainWindow()
    pressRealKey(`alt+${fileLetter}`)
    await expect.poll(() => state(tauriPage), { timeout: waitBudget(3000) }).toMatchObject({ menus: 1 })
    pressRealKey(await rowLetter(tauriPage, BAR_MENU, 'file_delete'))
    await expectNoDeletePrompt(tauriPage)
    await releaseMenus(tauriPage)

    // F10, ↓ into File, the arrows to Delete, and Enter.
    await focusField(tauriPage)
    focusMainWindow()
    pressRealKey('F10')
    pressRealKey('Down')
    await expect.poll(() => state(tauriPage), { timeout: waitBudget(3000) }).toMatchObject({ menus: 1 })
    await expect
      .poll(
        () => {
          pressRealKey('Down')
          return tauriPage.evaluate<string | null>(`(function(){
              var holder = document.getElementById(${JSON.stringify(FIELD_ID)});
              var id = holder && holder.getAttribute('aria-activedescendant');
              var row = id ? document.getElementById(id) : null;
              return row ? row.getAttribute('data-menu-id') : null;
          })()`)
        },
        { timeout: waitBudget(8000), intervals: [100] },
      )
      .toBe('file_delete')
    pressRealKey('Return')
    await expectNoDeletePrompt(tauriPage)
    await releaseMenus(tauriPage)

    // The control: the person's click on the same row opens the prompt.
    await focusField(tauriPage)
    const count = await tauriPage.evaluate<number>(`document.querySelectorAll(${JSON.stringify(TITLES)}).length`)
    expect(count).toBeGreaterThan(0)
    const title = await centerOf(tauriPage, TITLES)
    clickRealAt(mainWindowId(), title.x, title.y)
    await tauriPage.waitForSelector(`${BAR_MENU} [data-menu-id="file_delete"]`, waitBudget(3000))
    const row = await centerOf(tauriPage, `${BAR_MENU} [data-menu-id="file_delete"]`)
    clickRealAt(mainWindowId(), row.x, row.y)
    await tauriPage.waitForSelector(DELETE_DIALOG, waitBudget(5000))
  })

  test('the main-menu button’s own keys never run File > Delete from a text field, and a click on it does', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    const barShown = () => tauriPage.evaluate<boolean>(`document.querySelector('[role="menubar"]') !== null`)
    await dispatchMenuCommand(tauriPage, 'view.toggleMenuBar')
    try {
      await tauriPage.waitForSelector(MAIN_MENU_BUTTON, waitBudget(3000))
      expect(await moveCursorToFile(tauriPage, 'file-b.txt')).toBe(true)

      // F10 opens the button's menu (GNOME's key), then File's letter, then Delete's.
      await focusField(tauriPage)
      focusMainWindow()
      pressRealKey('F10')
      await expect.poll(() => state(tauriPage), { timeout: waitBudget(3000) }).toMatchObject({ menus: 1 })
      pressRealKey(await rowLetter(tauriPage, BUTTON_MENU, 'menu_file'))
      await expect.poll(() => state(tauriPage), { timeout: waitBudget(3000) }).toMatchObject({ menus: 2 })
      pressRealKey(await rowLetter(tauriPage, BUTTON_MENU, 'file_delete'))
      await expectNoDeletePrompt(tauriPage)
      await releaseMenus(tauriPage)

      // The control: real clicks on the button, File, and Delete open the prompt.
      await focusField(tauriPage)
      for (const selector of [
        MAIN_MENU_BUTTON,
        `${BUTTON_MENU} [data-menu-id="menu_file"]`,
        `${BUTTON_MENU} [data-menu-id="file_delete"]`,
      ]) {
        await tauriPage.waitForSelector(selector, waitBudget(3000))
        const at = await centerOf(tauriPage, selector)
        clickRealAt(mainWindowId(), at.x, at.y)
      }
      await tauriPage.waitForSelector(DELETE_DIALOG, waitBudget(5000))
    } finally {
      await cleanUp(tauriPage)
      if (!(await barShown())) await dispatchMenuCommand(tauriPage, 'view.toggleMenuBar')
      await expect.poll(barShown, { timeout: waitBudget(3000) }).toBe(true)
    }
  })
})

/** An element's center in page coordinates, which are the main window's on this lane. */
async function centerOf(page: PageLike, selector: string): Promise<{ x: number; y: number }> {
  const at = await page.evaluate<{ x: number; y: number } | null>(`(function(){
      var el = document.querySelector(${JSON.stringify(selector)});
      if (!el) return null;
      var r = el.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  })()`)
  if (!at) throw new Error(`nothing matches \`${selector}\``)
  return at
}
