/**
 * Characterization pins for keyboard focus in the main window: after each flow that moves
 * focus around (pane switches, view toggles, rename, dialogs, menus, the pane's list
 * swapping out and back), the next ↓ moves the cursor in the RIGHT pane, and only there.
 *
 * They exist so the screen-reader focus work (#437, #267) can move DOM focus onto the
 * focused pane's list without a single keystroke changing what it does. So they assert on
 * cursor movement (`.is-under-cursor` + `data-filename`), ❌ never on which element holds
 * focus or on `aria-selected`: both are free to change, the keys aren't. `expectFocusModel`
 * then checks the screen-reader side on its own: focus on the focused pane's list, naming
 * its cursor row.
 *
 * Every press here is synthetic and lands on `document.activeElement`
 * (`DETAILS.md` § "Keyboard: no press in this suite is a real one"). That's what makes a
 * focus LOSS visible: a ↓ dispatched on `<body>` never reaches the container's `onkeydown`.
 *
 * Both panes list `left/` so either cursor has somewhere to go; `ensureAppReady` puts the
 * right one back on `right/` for the next spec.
 *
 * Covered where the flow already lives, not repeated here: a toast closed with its X
 * (`app.spec.ts`), the Ask Cmdr rail's Escape (`ask-cmdr.spec.ts`), and search results
 * replacing a pane's list and going away again (`search-open-in-pane.spec.ts`).
 */

import { waitBudget } from './wait-budget.js'
import { test, expect } from './fixtures.js'
import { restoreFixtureTree } from '../e2e-shared/fixture-manifest.js'
import { ensureMcpClient, mcpCall } from '../e2e-shared/mcp-client.js'
import {
  clickEntryInPane,
  dismissOverlay,
  dispatchMenuCommand,
  ensureAppReady,
  executeViaCommandPalette,
  expectArrowDownMovesCursorIn,
  expectFocusModel,
  fileExistsInFocusedPane,
  focusPane,
  getFixtureRoot,
  moveCursorToFile,
  setRenameInput,
  CTRL_OR_META,
  LOCAL_VOLUME_NAME,
  MKDIR_DIALOG,
  type PageLike,
} from './helpers.js'

const MENU = '[data-menu]'

/** Points the right pane at `left/` too, then gives the left pane focus back. */
async function showLeftInBothPanes(tauriPage: PageLike): Promise<void> {
  const leftPath = `${getFixtureRoot()}/left`
  await tauriPage.evaluate(`(function() {
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'mcp-nav-to-path',
      payload: { pane: 'right', path: ${JSON.stringify(leftPath)} }
    });
  })()`)
  await expect
    .poll(
      async () =>
        tauriPage.evaluate<boolean>(
          `!!document.querySelectorAll('.file-pane')[1]?.querySelector('.file-entry[data-filename="file-a.txt"]')`,
        ),
      { timeout: waitBudget(5000) },
    )
    .toBeTruthy()
  await focusPane(tauriPage, 0)
}

/** The focused pane, from the class the store renders. */
async function focusedPane(tauriPage: PageLike): Promise<'left' | 'right' | null> {
  const index = await tauriPage.evaluate<number>(
    `Array.prototype.findIndex.call(document.querySelectorAll('.file-pane'), function(p) { return p.classList.contains('is-focused'); })`,
  )
  return index === 0 ? 'left' : index === 1 ? 'right' : null
}

/** Focus is on `pane`'s list, naming its cursor row, and ↓ moves `pane`'s cursor alone. */
async function expectKeysReach(tauriPage: PageLike, pane: 'left' | 'right'): Promise<void> {
  expect(await focusedPane(tauriPage)).toBe(pane)
  await expectFocusModel(tauriPage, '.file-pane.is-focused [role="listbox"]')
  await expectArrowDownMovesCursorIn(tauriPage, pane)
}

/** Opens the command palette with ⌘⇧P (Ctrl+Shift+P off macOS). */
async function openPalette(tauriPage: PageLike): Promise<void> {
  await tauriPage.evaluate(`document.dispatchEvent(new KeyboardEvent('keydown', {
    key: 'p', ctrlKey: ${String(CTRL_OR_META === 'Control')}, metaKey: ${String(CTRL_OR_META === 'Meta')}, shiftKey: true, bubbles: true
  }))`)
  await tauriPage.waitForSelector('.palette-overlay', waitBudget(3000))
}

test.afterEach(() => {
  // The rename pin commits a real rename; put `left/` back for the next spec.
  restoreFixtureTree(getFixtureRoot())
})

test.describe('Keyboard focus: which pane the next ↓ moves', () => {
  test.beforeEach(async ({ tauriPage }) => {
    await ensureAppReady(tauriPage)
    await showLeftInBothPanes(tauriPage)
  })

  test('after launch, the left pane', async ({ tauriPage }) => {
    await expectKeysReach(tauriPage, 'left')
  })

  test('after Tab, the right pane; after Tab again, the left', async ({ tauriPage }) => {
    await tauriPage.keyboard.press('Tab')
    await expect.poll(async () => focusedPane(tauriPage), { timeout: waitBudget(3000) }).toBe('right')
    await expectKeysReach(tauriPage, 'right')

    await tauriPage.keyboard.press('Tab')
    await expect.poll(async () => focusedPane(tauriPage), { timeout: waitBudget(3000) }).toBe('left')
    await expectKeysReach(tauriPage, 'left')
  })

  test('after toggling Full view and back to Brief, the same pane', async ({ tauriPage }) => {
    const startedInBrief = await tauriPage.evaluate<boolean>(
      `!!document.querySelector('.file-pane.is-focused .brief-list')`,
    )
    try {
      await dispatchMenuCommand(tauriPage, startedInBrief ? 'view.fullMode' : 'view.briefMode')
      await tauriPage.waitForSelector(
        `.file-pane.is-focused ${startedInBrief ? '.full-list' : '.brief-list'}`,
        waitBudget(3000),
      )
      await expectKeysReach(tauriPage, 'left')

      await dispatchMenuCommand(tauriPage, startedInBrief ? 'view.briefMode' : 'view.fullMode')
      await tauriPage.waitForSelector(
        `.file-pane.is-focused ${startedInBrief ? '.brief-list' : '.full-list'}`,
        waitBudget(3000),
      )
      await expectKeysReach(tauriPage, 'left')
    } finally {
      await dispatchMenuCommand(tauriPage, startedInBrief ? 'view.briefMode' : 'view.fullMode')
    }
  })

  test('after a rename commits, and after one is cancelled, the same pane', async ({ tauriPage }) => {
    expect(await moveCursorToFile(tauriPage, 'file-a.txt')).toBe(true)
    await dispatchMenuCommand(tauriPage, 'file.rename')
    await tauriPage.waitForSelector('.rename-input', waitBudget(3000))
    // Sorts first among the files, so the cursor still has rows below it.
    await setRenameInput(tauriPage, 'a-renamed.txt')
    await tauriPage.press('.rename-input', 'Enter')
    await expect
      .poll(async () => !(await tauriPage.isVisible('.rename-input')), { timeout: waitBudget(5000) })
      .toBeTruthy()
    await expect
      .poll(async () => fileExistsInFocusedPane(tauriPage, 'a-renamed.txt'), { timeout: waitBudget(5000) })
      .toBeTruthy()
    await expectKeysReach(tauriPage, 'left')

    expect(await moveCursorToFile(tauriPage, 'file-b.txt')).toBe(true)
    await dispatchMenuCommand(tauriPage, 'file.rename')
    await tauriPage.waitForSelector('.rename-input', waitBudget(3000))
    await tauriPage.press('.rename-input', 'Escape')
    await expect
      .poll(async () => !(await tauriPage.isVisible('.rename-input')), { timeout: waitBudget(5000) })
      .toBeTruthy()
    // file-b.txt is the last row; step back up so ↓ has somewhere to go.
    expect(await moveCursorToFile(tauriPage, 'a-renamed.txt')).toBe(true)
    await expectKeysReach(tauriPage, 'left')
  })

  test('after the new folder dialog closes, the same pane', async ({ tauriPage }) => {
    await dispatchMenuCommand(tauriPage, 'file.newFolder')
    await tauriPage.waitForSelector(MKDIR_DIALOG, waitBudget(3000))
    await dismissOverlay(tauriPage)
    await expectKeysReach(tauriPage, 'left')
  })

  test('after ⌥F1 then Escape, the same pane', async ({ tauriPage }) => {
    await dispatchMenuCommand(tauriPage, 'pane.leftVolumeChooser')
    await tauriPage.waitForSelector(MENU, waitBudget(3000))
    // Escape on the open menu IS the keyboard path under test, so it's a real press.
    await tauriPage.keyboard.press('Escape')
    await expect.poll(async () => tauriPage.count(MENU), { timeout: waitBudget(3000) }).toBe(0)
    await expectKeysReach(tauriPage, 'left')
  })

  test('after ⌥F1, Tab (the chooser moves panes), then Escape, the other pane', async ({ tauriPage }) => {
    await dispatchMenuCommand(tauriPage, 'pane.leftVolumeChooser')
    await tauriPage.waitForSelector(MENU, waitBudget(3000))
    await tauriPage.keyboard.press('Tab')
    await expect.poll(async () => focusedPane(tauriPage), { timeout: waitBudget(3000) }).toBe('right')
    await tauriPage.waitForSelector(MENU, waitBudget(3000))
    // The moved chooser keeps focus: the closing one's restorer must not pull it back to a list.
    await expect
      .poll(async () => tauriPage.evaluate<boolean>(`!!document.activeElement?.closest('${MENU}')`), {
        timeout: waitBudget(3000),
      })
      .toBe(true)
    // Escape on the open menu IS the keyboard path under test, so it's a real press.
    await tauriPage.keyboard.press('Escape')
    await expect.poll(async () => tauriPage.count(MENU), { timeout: waitBudget(3000) }).toBe(0)
    await expectKeysReach(tauriPage, 'right')
  })

  test('after the Favorites menu closes, the same pane', async ({ tauriPage }) => {
    await dispatchMenuCommand(tauriPage, 'favorites.open')
    await tauriPage.waitForSelector(MENU, waitBudget(3000))
    // Escape on the open menu IS the keyboard path under test, so it's a real press.
    await tauriPage.keyboard.press('Escape')
    await expect.poll(async () => tauriPage.count(MENU), { timeout: waitBudget(3000) }).toBe(0)
    await expectKeysReach(tauriPage, 'left')
  })

  test('after the pane shows the Servers list and comes back, the same pane', async ({ tauriPage }) => {
    await ensureMcpClient(tauriPage)
    try {
      await mcpCall('select_volume', { pane: 'left', name: 'Servers' })
      await expect
        .poll(async () => tauriPage.isVisible('.file-pane.is-focused [data-trailing-row]'), {
          timeout: waitBudget(15000),
        })
        .toBeTruthy()
    } finally {
      await mcpCall('select_volume', { pane: 'left', name: LOCAL_VOLUME_NAME })
    }
    await tauriPage.evaluate(`(function() {
      window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
        event: 'mcp-nav-to-path',
        payload: { pane: 'left', path: ${JSON.stringify(`${getFixtureRoot()}/left`)} }
      });
    })()`)
    await expect
      .poll(async () => fileExistsInFocusedPane(tauriPage, 'file-a.txt'), { timeout: waitBudget(5000) })
      .toBeTruthy()
    await expectKeysReach(tauriPage, 'left')
  })

  test('after the command palette runs "Switch pane", the right pane', async ({ tauriPage }) => {
    await executeViaCommandPalette(tauriPage, 'Switch pane')
    await expect.poll(async () => focusedPane(tauriPage), { timeout: waitBudget(3000) }).toBe('right')
    await expectKeysReach(tauriPage, 'right')
  })

  test('after the command palette closes with Escape, the same pane', async ({ tauriPage }) => {
    await openPalette(tauriPage)
    // Escape in the palette IS the keyboard path under test, so it's a real press.
    await tauriPage.keyboard.press('Escape')
    await expect.poll(async () => tauriPage.count('.palette-overlay'), { timeout: waitBudget(3000) }).toBe(0)
    await expectKeysReach(tauriPage, 'left')
  })

  // A synthetic click skips the browser's mousedown focus default, so this pins the cursor
  // outcome only. What VoiceOver hears on a REAL click is checked through the harness.
  test('after a click on a row in the unfocused pane, that pane', async ({ tauriPage }) => {
    await clickEntryInPane(tauriPage, 1, 1)
    await expect.poll(async () => focusedPane(tauriPage), { timeout: waitBudget(3000) }).toBe('right')
    await expectKeysReach(tauriPage, 'right')
  })

  // The PRESS alone, no click: a real click on empty list space never switched panes while
  // the switch waited for the click (the browser's focus default bounced through the old
  // pane first), so it happens on `mousedown`, as a row's does.
  test('after a press on empty list space in the unfocused pane, that pane', async ({ tauriPage }) => {
    const pressed = await tauriPage.evaluate<boolean>(`(function() {
      var surface = document.querySelectorAll('.file-pane')[1]?.querySelector('[data-file-list-surface]');
      if (!surface) return false;
      surface.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0 }));
      return true;
    })()`)
    expect(pressed).toBe(true)
    await expect.poll(async () => focusedPane(tauriPage), { timeout: waitBudget(3000) }).toBe('right')
    await expectKeysReach(tauriPage, 'right')
  })
})
