/**
 * Linux: the bar's keys and rows in the windows beside the main one (D5, task 63 of
 * `docs/specs/windows-port-plan.md`), with real X events only (`helpers/x11-keys.ts`).
 *
 * - Settings, the queue, and Keyboard shortcuts draw no bar off macOS, yet keep every key the
 *   main bar showed (`#lib/ui/command-menu/window-bar-keys.ts`): a real Ctrl+W closes each. Debug
 *   is a dev-build window, which the E2E build doesn't open; it runs the same root-layout code.
 * - The viewer draws its own bar: Ctrl+W (File > Close) closes it, Edit > Cut and Paste stay live
 *   while the menu is open over a focused search box, and View > Word wrap's check follows a pick.
 * - The main bar's View > Show hidden files check follows a pick.
 */
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test, expect } from './fixtures.js'
import { ensureAppReady, type PageLike } from './helpers.js'
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
  pressRealKey('Escape')
  await expect.poll(() => openMenus(page), { timeout: waitBudget(3000) }).toBe(0)
  if (!row) throw new Error(`row \`${menuId}\` vanished`)
  return row
}

test.describe('The bar in the other windows (Linux)', () => {
  test.skip(os.platform() !== 'linux', 'The page draws the bar off macOS; the Linux lane drives it with real X keys')

  test.afterEach(() => {
    focusMainWindow()
  })

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
      await expect
        .poll(
          () => viewer.page.evaluate<boolean>(`document.activeElement === document.querySelector('.search-bar input')`),
          {
            timeout: waitBudget(5000),
          },
        )
        .toBe(true)
      // Opening the menu moves focus to it; the rows still follow the box it came from.
      await openMenuHolding(viewer.page, viewer.windowId, 'viewer_edit_cut')
      await expect
        .poll(async () => (await rowState(viewer.page, 'viewer_edit_cut'))?.disabled, { timeout: waitBudget(3000) })
        .toBe(false)
      expect((await rowState(viewer.page, 'viewer_edit_paste'))?.disabled).toBe(false)
      pressRealKey('Escape')
      await expect.poll(() => openMenus(viewer.page), { timeout: waitBudget(3000) }).toBe(0)
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
})
