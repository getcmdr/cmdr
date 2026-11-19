/**
 * Windows' in-page right-click menus, end to end (task 63 step 7 of `docs/specs/windows-port-plan.md`):
 * a file row's menu by right-click, Shift+F10, and the Menu key, a pick that runs through the
 * backend's `pick_context_menu_item`, Escape with focus back on the list, and a text field's edit
 * menu in the inline rename editor, which must keep the rename alive.
 *
 * Only an app whose `contextMenus` convention answers `inPage` draws these, so every test skips
 * elsewhere. The Linux lane launches the app a second time with `CMDR_E2E_CONTEXT_MENUS=inPage`
 * (a `playwright-e2e`-only override, `src-tauri/src/platform/frontend.rs`) and runs `@in-page-menus`
 * against it (`scripts/e2e-linux.sh`). That's WebKitGTK, not WebView2: how WebView2 marks a
 * keyboard-raised `contextmenu`, and its user activation, stay for a Windows lane (task 55).
 *
 * The presses are synthetic (DETAILS § "Keyboard: no press in this suite is a real one"), so each
 * is dispatched by hand, `cancelable`, and a click follows Chromium and WebKit's compatibility rule:
 * a `preventDefault`ed `pointerdown` sends no `mousedown`.
 */

import { test, expect } from './fixtures.js'
import { ensureAppReady, moveCursorToFile } from './helpers.js'
import { waitBudget } from './wait-budget.js'
import type { PageLike } from './helpers.js'

const MENU = '[data-command-menu="context"]'
const FILE = 'file-a.txt'
const fileRow = `document.querySelector('.file-pane.is-focused .file-entry[data-filename="${FILE}"]')`

/** A pointer right-click on what `elementExpr` resolves to, at its middle. */
async function rightClick(page: PageLike, elementExpr: string): Promise<void> {
  const done = await page.evaluate<boolean>(`(function () {
        var el = ${elementExpr};
        if (!el) return false;
        var r = el.getBoundingClientRect();
        var o = { bubbles: true, cancelable: true, composed: true, button: 2, buttons: 2,
            clientX: r.left + Math.min(20, r.width / 2), clientY: r.top + r.height / 2,
            pointerId: 1, pointerType: 'mouse', isPrimary: true };
        el.dispatchEvent(new PointerEvent('pointerdown', o));
        el.dispatchEvent(new MouseEvent('mousedown', o));
        el.dispatchEvent(new PointerEvent('pointerup', o));
        el.dispatchEvent(new MouseEvent('mouseup', o));
        el.dispatchEvent(new PointerEvent('contextmenu', o));
        return true;
    })()`)
  expect(done).toBe(true)
}

/** A left click on a menu row the way the engine delivers it: no `mousedown` after a prevented `pointerdown`. */
async function clickRow(page: PageLike, menuId: string): Promise<void> {
  const done = await page.evaluate<boolean>(`(function () {
        var el = document.querySelector('${MENU} [data-menu-id="${menuId}"]');
        if (!el) return false;
        var r = el.getBoundingClientRect();
        var o = { bubbles: true, cancelable: true, composed: true, button: 0,
            clientX: r.left + r.width / 2, clientY: r.top + r.height / 2,
            pointerId: 1, pointerType: 'mouse', isPrimary: true };
        var down = new PointerEvent('pointerdown', o);
        el.dispatchEvent(down);
        if (!down.defaultPrevented) el.dispatchEvent(new MouseEvent('mousedown', o));
        el.dispatchEvent(new PointerEvent('pointerup', o));
        if (!down.defaultPrevented) el.dispatchEvent(new MouseEvent('mouseup', o));
        el.dispatchEvent(new MouseEvent('click', o));
        return true;
    })()`)
  expect(done).toBe(true)
}

/** A cancelable keydown on whatever has focus, as the keyboard sends it. */
async function key(page: PageLike, name: string, shiftKey = false): Promise<void> {
  await page.evaluate(`(function () {
        var target = document.activeElement || document.body;
        target.dispatchEvent(new KeyboardEvent('keydown', { key: ${JSON.stringify(name)}, shiftKey: ${String(shiftKey)},
            bubbles: true, cancelable: true, composed: true }));
    })()`)
}

const menuOpen = (page: PageLike) => page.evaluate<boolean>(`document.querySelector('${MENU}') !== null`)
const focusOnMenuRow = (page: PageLike) =>
  page.evaluate<boolean>(
    `document.activeElement?.closest('${MENU}') !== null && document.activeElement?.getAttribute('role') === 'menuitem'`,
  )
const focusInFocusedPane = (page: PageLike) =>
  page.evaluate<boolean>(`document.activeElement?.closest('.file-pane.is-focused') !== null`)

test.describe('in-page context menus @in-page-menus', () => {
  test.beforeEach(async ({ tauriPage }) => {
    const inPage = await tauriPage.evaluate<boolean>(`window.__CMDR_CONVENTIONS__?.contextMenus === 'inPage'`)
    test.skip(
      !inPage,
      'This app draws native right-click menus; the Linux lane runs these on a second launch with CMDR_E2E_CONTEXT_MENUS=inPage',
    )
    await ensureAppReady(tauriPage)
    expect(await moveCursorToFile(tauriPage, FILE)).toBe(true)
  })

  test.afterEach(async ({ tauriPage }) => {
    // Leave no menu or editor behind for the next spec.
    if (await menuOpen(tauriPage)) await key(tauriPage, 'Escape')
    if (await tauriPage.isVisible('.rename-input')) await tauriPage.press('.rename-input', 'Escape')
  })

  test("a right-click opens a file's menu, and its Rename pick runs through the backend", async ({ tauriPage }) => {
    await rightClick(tauriPage, fileRow)
    await expect.poll(() => menuOpen(tauriPage), { timeout: waitBudget(3000) }).toBeTruthy()
    await clickRow(tauriPage, 'rename')
    await expect.poll(() => tauriPage.isVisible('.rename-input'), { timeout: waitBudget(5000) }).toBeTruthy()
    expect(await menuOpen(tauriPage)).toBe(false)
  })

  for (const [name, keyName, shift] of [
    ['Shift+F10', 'F10', true],
    ['the Menu key', 'ContextMenu', false],
  ] as const) {
    test(`${name} opens the cursor row's menu on its first row, and Escape hands focus back to the list`, async ({
      tauriPage,
    }) => {
      await tauriPage.click('.file-pane.is-focused')
      await key(tauriPage, keyName, shift)
      await expect.poll(() => menuOpen(tauriPage), { timeout: waitBudget(3000) }).toBeTruthy()
      await expect.poll(() => focusOnMenuRow(tauriPage), { timeout: waitBudget(2000) }).toBeTruthy()
      await key(tauriPage, 'Escape')
      await expect.poll(async () => !(await menuOpen(tauriPage)), { timeout: waitBudget(2000) }).toBeTruthy()
      await expect.poll(() => focusInFocusedPane(tauriPage), { timeout: waitBudget(2000) }).toBeTruthy()
    })
  }

  test("the rename editor's edit menu keeps the rename alive: Select all selects the name, focus never leaves", async ({
    tauriPage,
  }) => {
    await tauriPage.keyboard.press('F2')
    await tauriPage.waitForSelector('.rename-input', waitBudget(3000))
    await tauriPage.evaluate(`(function () {
            var input = document.querySelector('.rename-input');
            input.focus();
            input.setSelectionRange(1, 1);
            window.__cmdrRenameBlurs = 0;
            input.addEventListener('blur', function () { window.__cmdrRenameBlurs++; });
        })()`)
    await rightClick(tauriPage, `document.querySelector('.rename-input')`)
    await expect.poll(() => menuOpen(tauriPage), { timeout: waitBudget(3000) }).toBeTruthy()
    expect(
      await tauriPage.evaluate<boolean>(`document.activeElement === document.querySelector('.rename-input')`),
    ).toBe(true)
    await clickRow(tauriPage, 'text_field_select_all')
    await expect
      .poll(
        () =>
          tauriPage.evaluate<boolean>(`(function () {
                var input = document.querySelector('.rename-input');
                return !!input && input.selectionStart === 0 && input.selectionEnd === input.value.length;
            })()`),
        { timeout: waitBudget(2000) },
      )
      .toBeTruthy()
    expect(await tauriPage.isVisible('.rename-input')).toBe(true)
    expect(await tauriPage.evaluate<number>(`window.__cmdrRenameBlurs`)).toBe(0)
    expect(await menuOpen(tauriPage)).toBe(false)
  })
})
