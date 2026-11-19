/**
 * Linux: the main window's in-page menu bar (D5, task 63 of `docs/specs/windows-port-plan.md`),
 * driven the way a person drives it, and the keys it must leave alone.
 *
 * Off macOS the page draws the bar (`routes/(main)/MainMenuBar.svelte` over `#lib/ui/MenuBar.svelte`)
 * and no GTK bar exists. Real X events only (`helpers/x11-keys.ts`): a synthetic DOM event would
 * reach the page whether or not GTK took the key first, so it can't prove GTK left a lone Alt or
 * F10 to the page. One language proves it: nothing here reads a label, the access letter comes off
 * the title's own `aria-keyshortcuts`.
 *
 * Also covered: while a dialog is up, a real `Alt+<letter>` (the search dialog's `Alt+F`, every
 * Multi-rename option key) reaches the dialog and opens no menu; the operation queue's
 * `Alt+↑` / `Alt+↓` and Settings' `Ctrl+A` / `X` / `V` / `C` reach their windows.
 */
import os from 'node:os'
import { test, expect } from './fixtures.js'
import {
  dispatchMenuCommand,
  ensureAppReady,
  escapeOverlayUntilGone,
  fileExistsInFocusedPane,
  focusPane,
  moveCursorToFile,
  pressKey,
  TRANSFER_DIALOG,
  type PageLike,
} from './helpers.js'
import { ensureMcpClient } from '../e2e-shared/mcp-client.js'
import {
  clickRealAt,
  F10_KEYCODE,
  focusMainWindow,
  focusWindow,
  focusWindowAtCorner,
  mainWindowId,
  pressRealKey,
  visibleTopLevelWindows,
} from './helpers/x11-keys.js'
import { closeScopedWindow, openQueueWindow, openSettingsWindowViaProd, QUEUE_WINDOW_LABEL } from './helpers/windows.js'
import type { TauriPage } from '@srsholmes/tauri-playwright'
import { defaultKey } from './helpers/registry-keys.js'
import { closeSearchDialog, getActiveMode, openSearchDialog, pollActiveMode, SEARCH_INPUT } from './search-helpers.js'
import { waitBudget } from './wait-budget.js'

const TITLES = '[role="menubar"] > [role="menuitem"]'
/** An open flyout of the bar (its submenus carry the same marker). */
const BAR_MENU = '[role="menu"][data-command-menu="bar"]'

/** What the bar shows right now, in one read: which title holds focus, which is open, and where focus is. */
interface BarState {
  /** The index of the title with DOM focus, or -1. */
  focusedTitle: number
  /** The index of the title whose menu is open, or -1. */
  openTitle: number
  /** How many bar flyouts are on screen. */
  menus: number
  /** Whether focus sits in the focused file pane. */
  focusInPane: boolean
}

function barState(page: PageLike): Promise<BarState> {
  return page.evaluate<BarState>(`(function(){
      var titles = Array.prototype.slice.call(document.querySelectorAll(${JSON.stringify(TITLES)}));
      var pane = document.querySelector('.file-pane.is-focused');
      return {
        focusedTitle: titles.indexOf(document.activeElement),
        openTitle: titles.findIndex(function(t){ return t.getAttribute('aria-expanded') === 'true'; }),
        menus: document.querySelectorAll(${JSON.stringify(BAR_MENU)}).length,
        focusInPane: !!(pane && pane.contains(document.activeElement)),
      };
  })()`)
}

/** Polls until the bar reads `expected` (the fields given). */
async function expectBar(page: PageLike, expected: Partial<BarState>): Promise<void> {
  await expect
    .poll(
      async () => {
        const state = await barState(page)
        return Object.fromEntries(Object.keys(expected).map((key) => [key, state[key as keyof BarState]]))
      },
      { timeout: waitBudget(3000) },
    )
    .toEqual(expected)
}

/** A title's center in page coordinates, which are the main window's on this lane. */
function titleCenter(page: PageLike, index: number): Promise<{ x: number; y: number }> {
  return page.evaluate<{ x: number; y: number }>(`(function(){
      var title = document.querySelectorAll(${JSON.stringify(TITLES)})[${String(index)}];
      var r = title.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  })()`)
}

/** A row's center by its menu id, in an open bar flyout. */
function rowCenter(page: PageLike, menuId: string): Promise<{ x: number; y: number } | null> {
  return page.evaluate<{ x: number; y: number } | null>(`(function(){
      var row = document.querySelector(${JSON.stringify(`${BAR_MENU} [data-menu-id="${menuId}"]`)});
      if (!row) return null;
      var r = row.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  })()`)
}

/** The index of the title whose menu holds `menuId`, opening each in turn by click to look. */
async function titleHolding(page: PageLike, menuId: string): Promise<number> {
  const count = await page.evaluate<number>(`document.querySelectorAll(${JSON.stringify(TITLES)}).length`)
  for (let index = 0; index < count; index++) {
    const at = await titleCenter(page, index)
    clickRealAt(mainWindowId(), at.x, at.y)
    await expectBar(page, { openTitle: index })
    if ((await rowCenter(page, menuId)) !== null) return index
    clickRealAt(mainWindowId(), at.x, at.y)
    await expectBar(page, { menus: 0 })
  }
  throw new Error(`no bar menu holds \`${menuId}\``)
}

/** The main-menu button at the end of the tab row, where no bar shows. */
const MAIN_MENU_BUTTON = '.main-menu-button'
/** The main-menu button's open menu (it rides the context-menu surfaces). */
const MAIN_MENU = '[role="menu"][data-command-menu="context"]'

/** Whether the main menu is open, and whether focus sits in the focused file pane. */
function mainMenuState(page: PageLike): Promise<{ open: boolean; focusInPane: boolean }> {
  return page.evaluate<{ open: boolean; focusInPane: boolean }>(`(function(){
      var pane = document.querySelector('.file-pane.is-focused');
      return {
        open: document.querySelector(${JSON.stringify(MAIN_MENU)}) !== null,
        focusInPane: !!(pane && pane.contains(document.activeElement)),
      };
  })()`)
}

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

/** Lets go of the bar, whatever it holds: a failed run can leave it with the keyboard. */
async function releaseBar(page: PageLike): Promise<void> {
  for (let tries = 0; tries < 4; tries++) {
    const state = await barState(page)
    if (state.focusedTitle < 0 && state.menus === 0) return
    focusMainWindow()
    pressRealKey('Escape')
  }
}

test.describe('The in-page menu bar (Linux)', () => {
  test.skip(os.platform() !== 'linux', 'The page draws the bar off macOS; the Linux lane drives it with real X keys')

  test.afterEach(async ({ tauriPage }) => {
    await releaseBar(tauriPage)
  })

  test('a lone Alt takes the bar, the arrows walk its menus, and Escape gives focus back to the pane', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    await focusPane(tauriPage, 0)
    await expectBar(tauriPage, { focusInPane: true, focusedTitle: -1, menus: 0 })

    focusMainWindow()
    pressRealKey('alt')
    await expectBar(tauriPage, { focusedTitle: 0, menus: 0 })

    pressRealKey('Right')
    await expectBar(tauriPage, { focusedTitle: 1, menus: 0 })
    pressRealKey('Left')
    await expectBar(tauriPage, { focusedTitle: 0 })

    pressRealKey('Down')
    await expectBar(tauriPage, { openTitle: 0, menus: 1 })
    pressRealKey('Right')
    await expectBar(tauriPage, { openTitle: 1, menus: 1 })
    pressRealKey('Left')
    await expectBar(tauriPage, { openTitle: 0, menus: 1 })

    // The two-Escape walk: the menu closes onto its title, then the bar lets go.
    pressRealKey('Escape')
    await expectBar(tauriPage, { openTitle: -1, menus: 0, focusedTitle: 0 })
    pressRealKey('Escape')
    await expectBar(tauriPage, { focusedTitle: -1, menus: 0, focusInPane: true })
  })

  test('F10 takes the bar and gives it back, and Alt plus a title’s letter opens that menu', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    await focusPane(tauriPage, 0)

    focusMainWindow()
    pressRealKey(F10_KEYCODE)
    await expectBar(tauriPage, { focusedTitle: 0, menus: 0 })
    pressRealKey(F10_KEYCODE)
    await expectBar(tauriPage, { focusedTitle: -1, focusInPane: true })

    // The second title's letter, as the bar announces it (`Alt+V`), so no label is read.
    const shortcut = await tauriPage.evaluate<string | null>(
      `document.querySelectorAll(${JSON.stringify(TITLES)})[1]?.getAttribute('aria-keyshortcuts') ?? null`,
    )
    expect(shortcut).toMatch(/^Alt\+.$/u)
    focusMainWindow()
    pressRealKey(`alt+${(shortcut ?? '').slice(-1).toLowerCase()}`)
    await expectBar(tauriPage, { openTitle: 1, menus: 1 })

    pressRealKey('Escape')
    pressRealKey('Escape')
    await expectBar(tauriPage, { focusedTitle: -1, menus: 0, focusInPane: true })
  })

  test('a click opens a menu, and a click on a row runs it', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    await focusPane(tauriPage, 0)
    const hiddenAtStart = await fileExistsInFocusedPane(tauriPage, '.hidden-file')
    const pickShowHidden = async () => {
      await titleHolding(tauriPage, 'show_hidden_files')
      const row = await rowCenter(tauriPage, 'show_hidden_files')
      expect(row).not.toBeNull()
      // A greyed row ignores a click: the page bar's rows stay live whatever window had focus last.
      expect(
        await tauriPage.evaluate<string | null>(
          `document.querySelector('${BAR_MENU} [data-menu-id="show_hidden_files"]')?.getAttribute('aria-disabled') ?? null`,
        ),
      ).toBeNull()
      if (row) clickRealAt(mainWindowId(), row.x, row.y)
      await expectBar(tauriPage, { menus: 0, focusedTitle: -1 })
    }
    try {
      await pickShowHidden()
      await expect
        .poll(() => fileExistsInFocusedPane(tauriPage, '.hidden-file'), { timeout: waitBudget(3000) })
        .toBe(!hiddenAtStart)
    } finally {
      // One app serves the shard, so the setting goes back as found.
      if ((await fileExistsInFocusedPane(tauriPage, '.hidden-file')) !== hiddenAtStart) await pickShowHidden()
    }
    await expect
      .poll(() => fileExistsInFocusedPane(tauriPage, '.hidden-file'), { timeout: waitBudget(3000) })
      .toBe(hiddenAtStart)
  })

  // Task 63 step 8: with "Show menu bar" off, Linux (this lane is the desktop-neutral row) shows the
  // main-menu button at the end of the tab row, F10 opens it, and its View submenu brings the bar back.
  test('View > Show menu bar hides the bar behind the main-menu button, and the button brings it back', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    await focusPane(tauriPage, 0)
    const barShown = () => tauriPage.evaluate<boolean>(`document.querySelector('[role="menubar"]') !== null`)
    try {
      await titleHolding(tauriPage, 'show_menu_bar')
      const row = await rowCenter(tauriPage, 'show_menu_bar')
      expect(row).not.toBeNull()
      if (row) clickRealAt(mainWindowId(), row.x, row.y)
      await expect.poll(barShown, { timeout: waitBudget(3000) }).toBe(false)
      await tauriPage.waitForSelector(MAIN_MENU_BUTTON, waitBudget(3000))

      // F10 opens the main menu where no bar takes it, and Escape gives focus back to the pane.
      await focusPane(tauriPage, 0)
      focusMainWindow()
      pressRealKey(F10_KEYCODE)
      await expect.poll(() => mainMenuState(tauriPage), { timeout: waitBudget(3000) }).toMatchObject({ open: true })
      pressRealKey('Escape')
      await expect
        .poll(() => mainMenuState(tauriPage), { timeout: waitBudget(3000) })
        .toEqual({ open: false, focusInPane: true })

      // A real click on the button, then View, then Show menu bar: the bar is back.
      const button = await centerOf(tauriPage, MAIN_MENU_BUTTON)
      clickRealAt(mainWindowId(), button.x, button.y)
      await expect.poll(() => mainMenuState(tauriPage), { timeout: waitBudget(3000) }).toMatchObject({ open: true })
      const view = await centerOf(tauriPage, `${MAIN_MENU} [data-menu-id="menu_view"]`)
      clickRealAt(mainWindowId(), view.x, view.y)
      await tauriPage.waitForSelector(`${MAIN_MENU} [data-menu-id="show_menu_bar"]`, waitBudget(3000))
      const showBar = await centerOf(tauriPage, `${MAIN_MENU} [data-menu-id="show_menu_bar"]`)
      clickRealAt(mainWindowId(), showBar.x, showBar.y)
      await expect.poll(barShown, { timeout: waitBudget(3000) }).toBe(true)
      expect(
        await tauriPage.evaluate<boolean>(`document.querySelector(${JSON.stringify(MAIN_MENU_BUTTON)}) === null`),
      ).toBe(true)
    } finally {
      // One app serves the shard, so the bar goes back as found.
      if (!(await barShown())) await dispatchMenuCommand(tauriPage, 'view.toggleMenuBar')
      await expect.poll(barShown, { timeout: waitBudget(3000) }).toBe(true)
    }
  })

  // Task 63 step 9: with a text field focused, the person's own click on File > Move runs it (the
  // page's bar knows a pick from a key), and the same command's key, F6, still doesn't.
  test('with a text field focused, a click on File > Move runs it, and its key still does not', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    expect(await moveCursorToFile(tauriPage, 'file-a.txt')).toBe(true)
    const fieldFocused = () =>
      tauriPage.evaluate<boolean>(`document.activeElement === document.getElementById('e2e-text-field-gate')`)
    try {
      // A text field the person is typing in, in the main window: the gate asks only that one has focus.
      await tauriPage.evaluate(`(function(){
          var field = document.createElement('input');
          field.id = 'e2e-text-field-gate';
          field.value = 'typing';
          document.body.append(field);
          field.focus();
      })()`)
      await expect.poll(fieldFocused, { timeout: waitBudget(3000) }).toBe(true)

      // The key: F6 reaches the page (the recorder sees it) and opens nothing.
      await installKeyRecorder(tauriPage)
      focusMainWindow()
      pressRealKey('F6')
      await expect.poll(() => keydownsSeen(tauriPage), { timeout: waitBudget(3000) }).toContain('F6')
      expect(
        await tauriPage.evaluate<boolean>(`document.querySelector(${JSON.stringify(TRANSFER_DIALOG)}) !== null`),
      ).toBe(false)
      expect(await fieldFocused()).toBe(true)

      // The pick: a real click on File, then on Move. The field keeps focus throughout.
      await titleHolding(tauriPage, 'file_move')
      expect(await fieldFocused()).toBe(true)
      const row = await rowCenter(tauriPage, 'file_move')
      expect(row).not.toBeNull()
      if (row) clickRealAt(mainWindowId(), row.x, row.y)
      await tauriPage.waitForSelector(TRANSFER_DIALOG, waitBudget(5000))
    } finally {
      await removeKeyRecorder(tauriPage)
      await escapeOverlayUntilGone(tauriPage, TRANSFER_DIALOG)
      await tauriPage.evaluate(`document.getElementById('e2e-text-field-gate')?.remove()`)
    }
  })

  test('a real Alt+F reaches the search dialog and opens no menu', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    await ensureMcpClient(tauriPage)
    try {
      await openSearchDialog(tauriPage)
      await tauriPage.evaluate(`(function(){
          var el = document.querySelector(${JSON.stringify(SEARCH_INPUT)});
          if (el) el.focus();
      })()`)
      await tauriPage.waitForFunction(
        `document.activeElement === document.querySelector(${JSON.stringify(SEARCH_INPUT)})`,
        waitBudget(3000),
      )
      // Start from Regex, so Filename mode can only come from the real Alt+F.
      await pressKey(tauriPage, 'Alt+r', 'KeyR')
      expect(await pollActiveMode(tauriPage, 'regex')).toBe(true)

      focusMainWindow()
      pressRealKey('alt+f')
      const switched = await pollActiveMode(tauriPage, 'filename', waitBudget(3000))
      expect({ switched, bar: await barState(tauriPage) }).toMatchObject({
        switched: true,
        bar: { menus: 0, focusedTitle: -1 },
      })
    } finally {
      if ((await getActiveMode(tauriPage)) !== null) await closeSearchDialog(tauriPage)
    }
  })

  test('every Multi-rename Alt+<letter> key reaches the sheet, from a text field and from an option', async ({
    tauriPage,
  }) => {
    await ensureAppReady(tauriPage)
    await ensureMcpClient(tauriPage)
    expect(await moveCursorToFile(tauriPage, 'file-a.txt')).toBe(true)
    try {
      for (const target of [SHEET_SEARCH_FIELD, SHEET_FIRST_OPTION]) {
        await dispatchMenuCommand(tauriPage, 'file.multiRename')
        await tauriPage.waitForSelector(`${SHEET} .sheet`, waitBudget(5000))
        await installKeyRecorder(tauriPage)

        // The toggles first, each pressed twice so the sheet ends as it began (it reopens on its
        // last settings); the two that open something, then Undo rename (nothing to undo), last.
        for (const commandId of [...SHEET_TOGGLES, ...SHEET_OPENERS]) {
          const chord = toXdotoolChord(defaultKey(commandId))
          const presses = SHEET_TOGGLES.includes(commandId) ? 2 : 1
          for (let press = 0; press < presses; press++) {
            const before = await optionStates(tauriPage)
            await focusIn(tauriPage, target)
            await tauriPage.evaluate('window.__keydownsSeen = []')
            focusMainWindow()
            pressRealKey(chord)
            await expect
              .poll(() => keydownsSeen(tauriPage), { timeout: waitBudget(3000) })
              .toContain(defaultKey(commandId))
            const after = {
              barMenus: (await barState(tauriPage)).menus,
              fieldText: await searchFieldText(tauriPage),
              toggled: (await optionStates(tauriPage)) !== before,
            }
            expect({ commandId, target, ...after }).toEqual({
              commandId,
              target,
              barMenus: 0,
              fieldText: '',
              toggled: SHEET_TOGGLES.includes(commandId),
            })
          }
        }

        // Go to parent's `Alt+↑` and Go back's `Alt+←` are the bar's rows' keys, and the sheet's rows
        // and fields use them: they must reach the sheet.
        for (const [chord, seen] of [
          ['alt+Up', 'Alt+ArrowUp'],
          ['alt+Left', 'Alt+ArrowLeft'],
        ] as const) {
          await focusIn(tauriPage, target)
          await tauriPage.evaluate('window.__keydownsSeen = []')
          focusMainWindow()
          pressRealKey(chord)
          await expect.poll(() => keydownsSeen(tauriPage), { timeout: waitBudget(3000) }).toContain(seen)
          expect((await barState(tauriPage)).menus).toBe(0)
        }
        await escapeOverlayUntilGone(tauriPage, SHEET)
      }
    } finally {
      if ((await tauriPage.count(SHEET)) > 0) await escapeOverlayUntilGone(tauriPage, SHEET)
      await removeKeyRecorder(tauriPage)
    }
  })

  // The queue's Move earlier is `Alt+↑`, which no bar may take from it.
  test('a real Alt+Up reaches the operation queue window', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    const main = tauriPage as TauriPage
    const atStart = visibleTopLevelWindows()
    const queue = await openQueueWindow(main)
    try {
      await queue.waitForSelector('.queue-window', waitBudget(5000))
      await expect.poll(() => newWindowsSince(atStart).length, { timeout: waitBudget(5000) }).toBe(1)
      const [queueWindow] = newWindowsSince(atStart)
      await installKeyRecorder(queue)
      for (const [chord, seen] of [
        ['alt+Down', 'Alt+ArrowDown'],
        ['alt+Up', 'Alt+ArrowUp'],
      ] as const) {
        await queue.evaluate('window.__keydownsSeen = []')
        focusWindow(queueWindow)
        pressRealKey(chord)
        await expect.poll(() => keydownsSeen(queue), { timeout: waitBudget(3000) }).toContain(seen)
      }
    } finally {
      await removeKeyRecorder(queue)
      await closeScopedWindow(main, queue, QUEUE_WINDOW_LABEL)
    }
  })

  // Settings has no bar of its own: Ctrl+X / C / V / A must still edit the focused field there.
  test('real Ctrl+A, X, V, and C edit a text field in the Settings window', async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    const main = tauriPage as TauriPage
    const atStart = visibleTopLevelWindows()
    const settings = await openSettingsWindowViaProd(main)
    const field = '.search-container input.text-field-control'
    const value = () =>
      settings.evaluate<string>(`document.querySelector(${JSON.stringify(field)})?.value ?? 'missing'`)
    const selection = () =>
      settings.evaluate<string>(`(function(){
          var el = document.querySelector(${JSON.stringify(field)});
          return el ? el.selectionStart + '-' + el.selectionEnd : 'missing';
      })()`)
    try {
      await settings.waitForSelector(field, waitBudget(8000))
      await expect.poll(() => newWindowsSince(atStart).length, { timeout: waitBudget(5000) }).toBe(1)
      const [settingsWindow] = newWindowsSince(atStart)
      // A window that just opened can still be taking focus: type one probe letter until it lands.
      await expect
        .poll(
          async () => {
            focusWindowAtCorner(settingsWindow)
            await settings.evaluate(`(function(){
                var el = document.querySelector(${JSON.stringify(field)});
                el.value = ''; el.focus();
            })()`)
            pressRealKey('x')
            return value()
          },
          { timeout: waitBudget(8000) },
        )
        .toBe('x')
      for (const letter of ['y', 'z']) pressRealKey(letter)
      await expect.poll(value, { timeout: waitBudget(3000) }).toBe('xyz')

      pressRealKey('ctrl+a')
      await expect.poll(selection, { timeout: waitBudget(3000) }).toBe('0-3')
      pressRealKey('ctrl+x')
      await expect.poll(value, { timeout: waitBudget(3000) }).toBe('')
      pressRealKey('ctrl+v')
      await expect.poll(value, { timeout: waitBudget(3000) }).toBe('xyz')
      pressRealKey('ctrl+a')
      pressRealKey('ctrl+c')
      pressRealKey('End')
      pressRealKey('ctrl+v')
      await expect.poll(value, { timeout: waitBudget(3000) }).toBe('xyzxyz')
    } finally {
      await settings.evaluate(`(function(){
          var el = document.querySelector(${JSON.stringify(field)});
          if (el) { el.value = ''; el.dispatchEvent(new Event('input', { bubbles: true })); }
      })()`)
      await closeScopedWindow(main, settings, 'settings')
    }
  })
})

/** The X windows that appeared since `before`: a new Cmdr window is one. */
function newWindowsSince(before: readonly string[]): string[] {
  return visibleTopLevelWindows().filter((id) => !before.includes(id))
}

const SHEET = '[data-dialog-id="multi-rename"]'
/** The Search field: the first plain field in the grid (the two masks sit in `.masks`). */
const SHEET_SEARCH_FIELD = `${SHEET} .field-grid > label.field input`
/** The first whole-name checkbox's input, which takes focus as the control. */
const SHEET_FIRST_OPTION = `${SHEET} .options .option-control input`

/** The eight option keys that flip a switch. */
const SHEET_TOGGLES: readonly string[] = [
  'multiRename.removeDiacritics',
  'multiRename.greekToLatin',
  'multiRename.normalizeUnicode',
  'multiRename.matchCase',
  'multiRename.firstMatchOnly',
  'multiRename.includeExtension',
  'multiRename.regex',
  'multiRename.replaceWholeName',
]
/** The other three: Letter case's menu, the media field picker, and Undo rename. */
const SHEET_OPENERS: readonly string[] = ['multiRename.letterCase', 'multiRename.insertField', 'multiRename.undoRename']

/** A Playwright chord (`Alt+r`) in xdotool's spelling (`alt+r`). */
function toXdotoolChord(chord: string): string {
  const XDOTOOL: Record<string, string> = { Control: 'ctrl', Alt: 'alt', Shift: 'shift', Meta: 'super' }
  return chord
    .split('+')
    .map((part) => XDOTOOL[part] ?? part)
    .join('+')
}

/** Records every keydown the page sees, as `defaultKey` spells it, so a key something else took shows as missing. */
async function installKeyRecorder(tauriPage: PageLike): Promise<void> {
  await tauriPage.evaluate(`(function(){
      window.__keydownsSeen = [];
      if (window.__keyRecorder) return;
      window.__keyRecorder = function(e){
        var mods = [];
        if (e.ctrlKey) mods.push('Control');
        if (e.altKey) mods.push('Alt');
        if (e.shiftKey) mods.push('Shift');
        if (e.metaKey) mods.push('Meta');
        var key = e.code.indexOf('Key') === 0 ? e.code.slice(3).toLowerCase() : e.key;
        window.__keydownsSeen.push(mods.concat([key]).join('+'));
      };
      window.addEventListener('keydown', window.__keyRecorder, true);
  })()`)
}

async function removeKeyRecorder(tauriPage: PageLike): Promise<void> {
  await tauriPage.evaluate(`(function(){
      if (window.__keyRecorder) window.removeEventListener('keydown', window.__keyRecorder, true);
      window.__keyRecorder = undefined;
      window.__keydownsSeen = undefined;
  })()`)
}

function keydownsSeen(tauriPage: PageLike): Promise<string[]> {
  return tauriPage.evaluate<string[]>('window.__keydownsSeen || []')
}

/** Every option's on/off state in the sheet, as one string. */
function optionStates(tauriPage: PageLike): Promise<string> {
  return tauriPage.evaluate<string>(`Array.prototype.map.call(
      document.querySelectorAll('${SHEET} [aria-pressed], ${SHEET} .checkbox-control[data-state]'),
      function(el){ return el.getAttribute('aria-pressed') || el.getAttribute('data-state'); }
  ).join(',')`)
}

function searchFieldText(tauriPage: PageLike): Promise<string> {
  return tauriPage.evaluate<string>(`document.querySelector(${JSON.stringify(SHEET_SEARCH_FIELD)})?.value ?? 'missing'`)
}

/**
 * Puts focus on `selector` and keeps asking until it stays there: the sheet places its own focus
 * once it has settled after opening (`open-settled.ts`), which can land after a first `focus()`.
 */
async function focusIn(tauriPage: PageLike, selector: string): Promise<void> {
  const sel = JSON.stringify(selector)
  await expect
    .poll(
      () =>
        tauriPage.evaluate<boolean>(`(function(){
            var el = document.querySelector(${sel});
            if (!el) return false;
            if (document.activeElement !== el) el.focus();
            return document.activeElement === el;
        })()`),
      { timeout: waitBudget(5000) },
    )
    .toBe(true)
}
