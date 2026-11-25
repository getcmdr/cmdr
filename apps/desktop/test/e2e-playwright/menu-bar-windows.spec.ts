/**
 * Linux: the bar's keys and rows in the windows beside the main one (D5, task 63 of
 * `docs/specs/windows-port-plan.md`), with real X events only (`helpers/x11-keys.ts`).
 *
 * - Settings, the queue, and Keyboard shortcuts draw no bar off macOS, yet keep every chord the
 *   main bar showed (`#lib/ui/command-menu/window-bar-keys.ts`): a real Ctrl+W closes each, and a
 *   real Tab and Shift+Tab still walk focus (the bar shows Tab for Switch pane, but only displays
 *   it). Debug is a dev-build window, which the E2E build doesn't open; it runs the same
 *   root-layout code.
 * - The viewer draws its own bar: Ctrl+W (File > Close) closes it, Edit > Cut and Paste stay live
 *   while the menu is open over a focused search box, and View > Word wrap's check follows a pick.
 * - The main bar's View > Show hidden files check follows a pick.
 * - The viewer's View > Show menu bar hides its bar, and its main-menu button brings it back.
 * - A real Ctrl+V in Settings, the queue, or the viewer never sends a paste to the main window: a
 *   page's pick names its window (`menu_handlers::PickWindow`).
 */
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test, expect } from './fixtures.js'
import { dispatchMenuCommand, ensureAppReady, moveCursorToFile, type PageLike } from './helpers.js'
import {
  clickRealAt,
  focusMainWindow,
  focusWindowAtCorner,
  mainWindowId,
  pressRealKey,
  visibleTopLevelWindows,
} from './helpers/x11-keys.js'
import { closeScopedWindow, openQueueWindow, openSettingsWindowViaProd, openViewerWindow } from './helpers/windows.js'
import type { TauriPage } from '@srsholmes/tauri-playwright'
import { defaultKey } from './helpers/registry-keys.js'
import { waitBudget } from './wait-budget.js'

const TITLES = '[role="menubar"] > [role="menuitem"]'
const BAR_MENU = '[role="menu"][data-command-menu="bar"]'
/** The viewer's search box. */
const SEARCH_BOX = '.search-bar input.text-field-control'

/** The X windows that appeared since `before`. */
function newWindowsSince(before: readonly string[]): string[] {
  return visibleTopLevelWindows().filter((id) => !before.includes(id))
}

async function windowLabels(main: TauriPage): Promise<string[]> {
  return (await main.listWindows()).map((w) => w.label)
}

/** Puts real keyboard focus in `windowId`, polling until its page says it has it. */
async function focusChild(child: PageLike, windowId: string): Promise<void> {
  await expect
    .poll(
      async () => {
        focusWindowAtCorner(windowId)
        return child.evaluate<boolean>('document.hasFocus()')
      },
      { timeout: waitBudget(8000) },
    )
    .toBe(true)
}

/**
 * Presses a real Ctrl+W in the child window until it closes. Each press waits for the child to
 * hold focus first, so a press can't land in the main window and close a tab there. Retried
 * because a window that just opened may not have its bar's rows yet.
 */
async function closeWithCtrlW(main: TauriPage, child: PageLike, windowId: string, label: string): Promise<void> {
  await expect
    .poll(
      async () => {
        if (!(await windowLabels(main)).includes(label)) return true
        await focusChild(child, windowId)
        pressRealKey('ctrl+w')
        return !(await windowLabels(main)).includes(label)
      },
      { timeout: waitBudget(10000), intervals: [500] },
    )
    .toBe(true)
}

/** Opens a child window with `open`, returning it with its X window id and its label. */
async function openChild(
  main: TauriPage,
  open: () => Promise<TauriPage>,
): Promise<{ page: TauriPage; windowId: string; label: string }> {
  const xBefore = visibleTopLevelWindows()
  const labelsBefore = await windowLabels(main)
  const page = await open()
  await expect.poll(() => newWindowsSince(xBefore).length, { timeout: waitBudget(5000) }).toBe(1)
  const [windowId] = newWindowsSince(xBefore)
  const label = (await windowLabels(main)).find((l) => !labelsBefore.includes(l)) ?? ''
  expect(label).not.toBe('')
  return { page, windowId, label }
}

function openShortcutsWindow(main: TauriPage): Promise<TauriPage> {
  return main
    .evaluate(
      `window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'execute-command', payload: { commandId: 'help.openShortcuts' } })`,
    )
    .then(() => main.waitForWindow((w) => w.label === 'shortcuts', { timeout: waitBudget(10000) }))
}

const fixtureRoot = (() => {
  const root = process.env.CMDR_E2E_START_PATH
  if (!root)
    throw new Error('CMDR_E2E_START_PATH env var is not set; fixtures must be created before running this spec')
  return root
})()

/** A text file the viewer opens, in the lane's fixture tree. */
function viewerFile(): string {
  const file = path.join(fixtureRoot, 'left', 'file-a.txt')
  if (!fs.existsSync(file)) throw new Error(`missing viewer fixture: ${file}`)
  return file
}

/** A title's center in its window's page coordinates. */
function titleCenter(page: PageLike, index: number): Promise<{ x: number; y: number }> {
  return page.evaluate<{ x: number; y: number }>(`(function(){
      var title = document.querySelectorAll(${JSON.stringify(TITLES)})[${String(index)}];
      var r = title.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  })()`)
}

/** The bar flyout row `menuId`'s center and state, or `null` while no open flyout holds it. */
function rowState(
  page: PageLike,
  menuId: string,
): Promise<{ x: number; y: number; disabled: boolean; checked: string | null } | null> {
  return page.evaluate(`(function(){
      var row = document.querySelector(${JSON.stringify(`${BAR_MENU} [data-menu-id="${menuId}"]`)});
      if (!row) return null;
      var r = row.getBoundingClientRect();
      return {
        x: r.left + r.width / 2, y: r.top + r.height / 2,
        disabled: row.getAttribute('aria-disabled') === 'true',
        checked: row.getAttribute('aria-checked'),
      };
  })()`)
}

function openMenus(page: PageLike): Promise<number> {
  return page.evaluate<number>(`document.querySelectorAll(${JSON.stringify(BAR_MENU)}).length`)
}

/** Opens the bar menu holding `menuId` by real clicks, one title at a time, and returns its title index. */
async function openMenuHolding(page: PageLike, windowId: string, menuId: string): Promise<number> {
  const count = await page.evaluate<number>(`document.querySelectorAll(${JSON.stringify(TITLES)}).length`)
  for (let index = 0; index < count; index++) {
    const at = await titleCenter(page, index)
    clickRealAt(windowId, at.x, at.y)
    await expect.poll(() => openMenus(page), { timeout: waitBudget(3000) }).toBeGreaterThan(0)
    if ((await rowState(page, menuId)) !== null) return index
    pressRealKey('Escape')
    await expect.poll(() => openMenus(page), { timeout: waitBudget(3000) }).toBe(0)
  }
  throw new Error(`no bar menu holds \`${menuId}\``)
}

/**
 * Lets go of the bar: Escape closes the open menu, and a second gives the keyboard back (the Win32
 * two-Escape walk), so the next key reaches the page, not a title.
 */
async function releaseBar(page: PageLike): Promise<void> {
  const holds = () =>
    page.evaluate<boolean>(`(function(){
        var titles = Array.prototype.slice.call(document.querySelectorAll(${JSON.stringify(TITLES)}));
        return document.querySelectorAll(${JSON.stringify(BAR_MENU)}).length > 0 || titles.indexOf(document.activeElement) >= 0;
    })()`)
  for (let tries = 0; tries < 4 && (await holds()); tries++) pressRealKey('Escape')
  await expect.poll(holds, { timeout: waitBudget(3000) }).toBe(false)
}

/** Picks row `menuId` by a real click, from a closed bar. */
async function pickRow(page: PageLike, windowId: string, menuId: string): Promise<void> {
  await openMenuHolding(page, windowId, menuId)
  const row = await rowState(page, menuId)
  expect(row).not.toBeNull()
  if (row) clickRealAt(windowId, row.x, row.y)
  await expect.poll(() => openMenus(page), { timeout: waitBudget(3000) }).toBe(0)
}

/** Reads row `menuId`'s state with its menu opened, then closes the menu. */
async function readRow(
  page: PageLike,
  windowId: string,
  menuId: string,
): Promise<{ disabled: boolean; checked: string | null }> {
  await openMenuHolding(page, windowId, menuId)
  const row = await rowState(page, menuId)
  await releaseBar(page)
  if (!row) throw new Error(`row \`${menuId}\` vanished`)
  return row
}

test.describe('The bar in the other windows (Linux)', () => {
  test.skip(os.platform() !== 'linux', 'The page draws the bar off macOS; the Linux lane drives it with real X keys')

  test.afterEach(() => {
    focusMainWindow()
  })

  // Regression anchor (review of #1271): the window fired Switch pane's Tab, so focus never moved.
  // The queue starts empty, with nothing to Tab to, so there only the key's fate is checked.
  for (const [name, open, walks] of [
    ['Settings', openSettingsWindowViaProd, true],
    ['the operation queue', openQueueWindow, false],
    ['Keyboard shortcuts', openShortcutsWindow, true],
  ] as const) {
    test(`a real Tab and Shift+Tab reach ${name}'s page, and Tab walks its focus`, async ({ tauriPage }) => {
      await ensureAppReady(tauriPage)
      const main = tauriPage as TauriPage
      const child = await openChild(main, () => open(main))
      try {
        await focusChild(child.page, child.windowId)
        // Start on the first focusable that isn't a text field (a field takes Tab as its own anyway),
        // or on the page itself where nothing is. The recorder listens last, on the window, so it
        // sees whether anything before it (the bar's keys among them) took the key.
        const startOnFirstFocusable = `(function(){
            var all = document.querySelectorAll('button:not([disabled]), [tabindex="0"], a[href]');
            for (var i = 0; i < all.length; i++) {
              var el = all[i];
              if (el.offsetParent === null) continue;
              el.focus();
              if (document.activeElement === el) return true;
            }
            return false;
        })()`
        // A window that walks has rendered its controls first.
        if (walks)
          await expect
            .poll(() => child.page.evaluate<boolean>(startOnFirstFocusable), { timeout: waitBudget(8000) })
            .toBe(true)
        await child.page.evaluate(`(function(){
            ${walks ? '' : `${startOnFirstFocusable};`}
            window.__cmdrTabStart = document.activeElement;
            window.__cmdrTabKeys = [];
            window.addEventListener('keydown', function (e) {
              // WebKitGTK names Shift+Tab (X's ISO_Left_Tab) 'Unidentified'.
              if (e.key === 'Tab' || e.key === 'Unidentified') window.__cmdrTabKeys.push(e.defaultPrevented);
            });
        })()`)
        const keys = () => child.page.evaluate<boolean[]>('window.__cmdrTabKeys')
        pressRealKey('Tab')
        await expect.poll(async () => (await keys()).length, { timeout: waitBudget(3000) }).toBe(1)
        expect(await keys()).toEqual([false])
        if (walks)
          await expect
            .poll(
              () =>
                child.page.evaluate<string>(`(function(){
                    var a = document.activeElement, s = window.__cmdrTabStart;
                    if (a !== s) return 'moved';
                    return 'still on ' + (s ? s.tagName + '.' + s.className + ' ' + s.textContent.trim().slice(0, 30) : 'nothing');
                })()`),
              { timeout: waitBudget(3000) },
            )
            .toBe('moved')
        pressRealKey('shift+Tab')
        await expect.poll(async () => (await keys()).length, { timeout: waitBudget(3000) }).toBe(2)
        // Neither key was taken from the page: the browser moved focus.
        expect(await keys()).toEqual([false, false])
      } finally {
        if ((await windowLabels(main)).includes(child.label)) await closeScopedWindow(main, child.page, child.label)
      }
    })
  }

  // Regression anchor (review of #1265): with no native bar, Ctrl+W stopped closing these windows.
  for (const [name, open] of [
    ['Settings', openSettingsWindowViaProd],
    ['the operation queue', openQueueWindow],
    ['Keyboard shortcuts', openShortcutsWindow],
  ] as const) {
    test(`a real Ctrl+W closes ${name}, which draws no bar`, async ({ tauriPage }) => {
      await ensureAppReady(tauriPage)
      const main = tauriPage as TauriPage
      const child = await openChild(main, () => open(main))
      try {
        expect(await child.page.evaluate<number>(`document.querySelectorAll('[role="menubar"]').length`)).toBe(0)
        await closeWithCtrlW(main, child.page, child.windowId, child.label)
      } finally {
        if ((await windowLabels(main)).includes(child.label)) await closeScopedWindow(main, child.page, child.label)
      }
    })
  }

  test('a real Ctrl+W closes the viewer, as its bar’s File > Close shows', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    const main = tauriPage as TauriPage
    const viewer = await openChild(main, () => openViewerWindow(main, viewerFile()))
    try {
      await viewer.page.waitForSelector('[role="menubar"]', waitBudget(8000))
      await closeWithCtrlW(main, viewer.page, viewer.windowId, viewer.label)
    } finally {
      if ((await windowLabels(main)).includes(viewer.label)) await closeScopedWindow(main, viewer.page, viewer.label)
    }
  })

  test('the viewer bar’s Cut and Paste stay live while its menu is open over the focused search box', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    const main = tauriPage as TauriPage
    const viewer = await openChild(main, () => openViewerWindow(main, viewerFile()))
    try {
      await viewer.page.waitForSelector('[role="menubar"]', waitBudget(8000))
      await focusChild(viewer.page, viewer.windowId)
      // Greyed with the search box closed.
      expect((await readRow(viewer.page, viewer.windowId, 'viewer_edit_cut')).disabled).toBe(true)
      await viewer.page.keyboard.press(defaultKey('viewer.search'))
      await viewer.page.waitForSelector(SEARCH_BOX, waitBudget(5000))
      // Focus the box itself until it holds focus: the bar opened just before may still be handing it back.
      await expect
        .poll(
          () =>
            viewer.page.evaluate<boolean>(`(function(){
                var el = document.querySelector(${JSON.stringify(SEARCH_BOX)});
                if (el && document.activeElement !== el) el.focus();
                return !!el && document.activeElement === el;
            })()`),
          { timeout: waitBudget(5000) },
        )
        .toBe(true)
      // Opening the menu moves focus to it; the rows still follow the box it came from.
      await openMenuHolding(viewer.page, viewer.windowId, 'viewer_edit_cut')
      await expect
        .poll(async () => (await rowState(viewer.page, 'viewer_edit_cut'))?.disabled, { timeout: waitBudget(3000) })
        .toBe(false)
      expect((await rowState(viewer.page, 'viewer_edit_paste'))?.disabled).toBe(false)
      await releaseBar(viewer.page)
    } finally {
      await closeScopedWindow(main, viewer.page, viewer.label)
    }
  })

  test('the viewer bar’s Word wrap check follows a pick', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    const main = tauriPage as TauriPage
    const viewer = await openChild(main, () => openViewerWindow(main, viewerFile()))
    try {
      await viewer.page.waitForSelector('[role="menubar"]', waitBudget(8000))
      await focusChild(viewer.page, viewer.windowId)
      const before = (await readRow(viewer.page, viewer.windowId, 'viewer_word_wrap')).checked
      expect(before === 'true' || before === 'false').toBe(true)
      const flip = async () => {
        await pickRow(viewer.page, viewer.windowId, 'viewer_word_wrap')
      }
      try {
        await flip()
        await expect
          .poll(async () => (await readRow(viewer.page, viewer.windowId, 'viewer_word_wrap')).checked, {
            timeout: waitBudget(5000),
          })
          .toBe(before === 'true' ? 'false' : 'true')
      } finally {
        // Word wrap persists: put it back as found.
        if ((await readRow(viewer.page, viewer.windowId, 'viewer_word_wrap')).checked !== before) await flip()
      }
    } finally {
      await closeScopedWindow(main, viewer.page, viewer.label)
    }
  })

  test('the main bar’s Show hidden files check follows a pick', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    focusMainWindow()
    const main = mainWindowId()
    const before = (await readRow(tauriPage, main, 'show_hidden_files')).checked
    expect(before === 'true' || before === 'false').toBe(true)
    try {
      await pickRow(tauriPage, main, 'show_hidden_files')
      await expect
        .poll(async () => (await readRow(tauriPage, main, 'show_hidden_files')).checked, { timeout: waitBudget(5000) })
        .toBe(before === 'true' ? 'false' : 'true')
    } finally {
      // One app serves the shard, so the setting goes back as found.
      if ((await readRow(tauriPage, main, 'show_hidden_files')).checked !== before)
        await pickRow(tauriPage, main, 'show_hidden_files')
    }
  })

  // Regression anchors (review of #1271): the bar's menu took focus from the rename field, whose blur
  // ended the rename; and Paste read the clipboard through a backend that said "not supported".
  // #1464: the bar taking the keyboard re-ran an explorer focus effect that pulled focus from the
  // field into the list, so the rename ended and Cut cut the file (about one run in 12 after the
  // multi-window test below); no wait here can paper over that, the product must not do it.
  test('the main bar’s Edit > Cut and Paste edit the inline rename’s name, which stays open', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    focusMainWindow()
    const main = mainWindowId()
    await tauriPage.click('.file-pane.is-focused')
    expect(await moveCursorToFile(tauriPage, 'file-a.txt')).toBe(true)
    await tauriPage.keyboard.press('F2')
    await tauriPage.waitForSelector('.rename-input', waitBudget(5000))
    const value = () =>
      tauriPage.evaluate<string>(`(function(){
          var input = document.querySelector('.rename-input');
          return input ? input.value : 'gone';
      })()`)
    try {
      const before = await value()
      await tauriPage.evaluate(`(function(){
          var input = document.querySelector('.rename-input');
          input.focus();
          input.setSelectionRange(0, input.value.length);
      })()`)
      await pickRow(tauriPage, main, 'edit_cut')
      await expect.poll(value, { timeout: waitBudget(3000) }).toBe('')
      await pickRow(tauriPage, main, 'edit_paste')
      await expect.poll(value, { timeout: waitBudget(3000) }).toBe(before)
      expect(
        await tauriPage.evaluate<boolean>(`document.activeElement === document.querySelector('.rename-input')`),
      ).toBe(true)
    } finally {
      if (await tauriPage.isVisible('.rename-input')) await tauriPage.press('.rename-input', 'Escape')
    }
  })

  test('the viewer bar’s Edit > Cut and Paste edit the search box', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    const main = tauriPage as TauriPage
    const viewer = await openChild(main, () => openViewerWindow(main, viewerFile()))
    const box = () =>
      viewer.page.evaluate<string>(`(function(){
          var el = document.querySelector(${JSON.stringify(SEARCH_BOX)});
          return el ? el.value : 'gone';
      })()`)
    try {
      await viewer.page.waitForSelector('[role="menubar"]', waitBudget(8000))
      await focusChild(viewer.page, viewer.windowId)
      await viewer.page.keyboard.press(defaultKey('viewer.search'))
      await viewer.page.waitForSelector(SEARCH_BOX, waitBudget(5000))
      await expect
        .poll(
          () =>
            viewer.page.evaluate<boolean>(`(function(){
                var el = document.querySelector(${JSON.stringify(SEARCH_BOX)});
                if (el && document.activeElement !== el) el.focus();
                return !!el && document.activeElement === el;
            })()`),
          { timeout: waitBudget(5000) },
        )
        .toBe(true)
      for (const key of ['x', 'y', 'z']) pressRealKey(key)
      await expect.poll(box, { timeout: waitBudget(3000) }).toBe('xyz')
      await viewer.page.evaluate(`document.querySelector(${JSON.stringify(SEARCH_BOX)}).select()`)
      await pickRow(viewer.page, viewer.windowId, 'viewer_edit_cut')
      await expect.poll(box, { timeout: waitBudget(3000) }).toBe('')
      await pickRow(viewer.page, viewer.windowId, 'viewer_edit_paste')
      await expect.poll(box, { timeout: waitBudget(3000) }).toBe('xyz')
    } finally {
      await closeScopedWindow(main, viewer.page, viewer.label)
    }
  })

  // Review of #1281: the viewer's bar had no Show menu bar row, so a viewer could neither hide its
  // bar nor get it back; the main window had to.
  test('the viewer bar’s View > Show menu bar hides its bar, and its main-menu button brings it back', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    const main = tauriPage as TauriPage
    const viewer = await openChild(main, () => openViewerWindow(main, viewerFile()))
    const shows = (page: PageLike) =>
      page.evaluate<{ bar: boolean; button: boolean }>(`({
          bar: document.querySelector('[role="menubar"]') !== null,
          button: document.querySelector(${JSON.stringify(MAIN_MENU_BUTTON)}) !== null,
      })`)
    try {
      await viewer.page.waitForSelector('[role="menubar"]', waitBudget(8000))
      await focusChild(viewer.page, viewer.windowId)
      await pickRow(viewer.page, viewer.windowId, 'show_menu_bar')
      await expect.poll(() => shows(viewer.page), { timeout: waitBudget(5000) }).toEqual({ bar: false, button: true })
      // The setting is the app's, so the main window follows too.
      await expect.poll(() => shows(main), { timeout: waitBudget(5000) }).toEqual({ bar: false, button: true })

      // Back from the viewer's own button: its View submenu carries the same row.
      for (const selector of [
        MAIN_MENU_BUTTON,
        `${MAIN_MENU} [data-menu-id="menu_view"]`,
        `${MAIN_MENU} [data-menu-id="show_menu_bar"]`,
      ]) {
        await viewer.page.waitForSelector(selector, waitBudget(3000))
        const at = await centerOf(viewer.page, selector)
        clickRealAt(viewer.windowId, at.x, at.y)
      }
      await expect.poll(() => shows(viewer.page), { timeout: waitBudget(5000) }).toEqual({ bar: true, button: false })
    } finally {
      if (!(await shows(main)).bar) await dispatchMenuCommand(main, 'view.toggleMenuBar')
      await expect.poll(async () => (await shows(main)).bar, { timeout: waitBudget(3000) }).toBe(true)
      await closeScopedWindow(main, viewer.page, viewer.label)
    }
  })

  // Review of #1281: a Ctrl+V in the queue, read against a stale "main is focused", ran the main
  // window's file paste. A page's pick names its window now, so another window's Edit rows edit
  // that window's own field and never reach the main window's files.
  test('a real Ctrl+V in Settings, the queue, or the viewer never sends a paste to the main window', async ({
    tauriPage,
  }) => {
    // Three child windows, each opened, focused, and closed.
    test.setTimeout(waitBudget(90_000))
    await ensureAppReady(tauriPage)
    const main = tauriPage as TauriPage
    expect(await moveCursorToFile(main, 'file-a.txt')).toBe(true)
    const probe = await installCommandProbe(main)
    try {
      for (const open of [
        () => openSettingsWindowViaProd(main),
        () => openQueueWindow(main),
        () => openViewerWindow(main, viewerFile()),
      ]) {
        const child = await openChild(main, open)
        try {
          await focusChild(child.page, child.windowId)
          // The child sees the key, so the press really landed there.
          await child.page.evaluate(`(function(){
              window.__ctrlVSeen = 0;
              window.addEventListener('keydown', function(e){ if (e.ctrlKey && e.key === 'v') window.__ctrlVSeen++; }, true);
          })()`)
          pressRealKey('ctrl+v')
          await expect
            .poll(() => child.page.evaluate<number>('window.__ctrlVSeen'), { timeout: waitBudget(3000) })
            .toBeGreaterThan(0)
          // The relay, were there one, arrives within the same round trip as the pick.
          await expect.poll(() => commandsSeen(main), { timeout: waitBudget(1500) }).not.toContain('edit.paste')
        } finally {
          await closeScopedWindow(main, child.page, child.label)
        }
      }
      expect(await commandsSeen(main)).not.toContain('edit.paste')
    } finally {
      await removeCommandProbe(main, probe)
    }
  })
})

/** The main-menu button and its open menu (it rides the context-menu surfaces). */
const MAIN_MENU_BUTTON = '.main-menu-button'
const MAIN_MENU = '[role="menu"][data-command-menu="context"]'

/** An element's center in its window's page coordinates. */
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

/** Records every `execute-command` the main window hears, by command id; returns the listener's event id. */
async function installCommandProbe(main: TauriPage): Promise<number> {
  return main.evaluate<number>(`(async function() {
    window.__commandsSeen = [];
    var handler = window.__TAURI_INTERNALS__.transformCallback(function(e) {
      window.__commandsSeen.push(e && e.payload ? e.payload.commandId : '?');
    });
    return await window.__TAURI_INTERNALS__.invoke('plugin:event|listen', {
      event: 'execute-command', target: { kind: 'Any' }, handler: handler
    });
  })()`)
}

function commandsSeen(main: TauriPage): Promise<string[]> {
  return main.evaluate<string[]>('window.__commandsSeen || []')
}

async function removeCommandProbe(main: TauriPage, eventId: number): Promise<void> {
  await main.evaluate(`(async function() {
    try { window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener('execute-command', ${String(eventId)}); } catch (e) {}
    await window.__TAURI_INTERNALS__.invoke('plugin:event|unlisten', { event: 'execute-command', eventId: ${String(eventId)} });
    window.__commandsSeen = undefined;
  })()`)
}
