/**
 * The menus a page draws (D5, task 63 of `docs/specs/windows-port-plan.md`): the in-window menu
 * bar, the viewer's bar, and Windows' in-page right-click menus, all answered by the backend from
 * `cmdr-menu` (`src-tauri/src/menu/DETAILS.md` § "The bar a page draws" and § "Right-click menus a
 * page draws"). `#lib/ui/MenuBar.svelte` and `ContextMenu.svelte` draw what these answer.
 */

import { type UnlistenFn } from '@tauri-apps/api/event'
import {
  commands,
  events,
  type BarMenuView,
  type BarPickOutcome,
  type ContextPickOutcome,
  type InPageContextMenu,
  type InPageContextMenuRequest,
  type MenuBarChanged,
  type ViewerBarInputs,
} from '#lib/ipc/bindings.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { applyMenuBarChange } from '#lib/ui/command-menu/bar-change.js'

const log = getAppLogger('inPageMenus')

/** A pick from the in-page bar (the main bar's or the viewer's), run by the handler a native click reaches. */
export async function pickMenuBarItem(menuId: string): Promise<BarPickOutcome> {
  return commands.pickMenuBarItem(menuId)
}

/** The viewer window's bar from its own state; asked again whenever one of `inputs` moves (no event). */
export async function getViewerMenuBar(inputs: ViewerBarInputs): Promise<BarMenuView[]> {
  return commands.getViewerMenuBar(inputs)
}

/**
 * One right-click menu's rows for the page to draw, with the facts its picks need stashed first,
 * as the native `show_*` command does. Ask where `convention('contextMenus')` is `inPage`.
 */
export async function openContextMenuInPage(request: InPageContextMenuRequest): Promise<InPageContextMenu> {
  return commands.openContextMenuInPage(request)
}

/** A pick from the in-page right-click menu `token` names: `stale` when another menu opened since. */
export async function pickContextMenuItem(token: number, menuId: string): Promise<ContextPickOutcome> {
  return commands.pickContextMenuItem(token, menuId)
}

/** The first retry of a failed `get_menu_bar`, doubling per failure up to {@link MAX_RETRY_MS}. */
const FIRST_RETRY_MS = 250
const MAX_RETRY_MS = 5_000

/**
 * Follows the main window's bar: calls `onMenus` with the whole bar now and after every change.
 * The ordering contract (`menu/DETAILS.md`): subscribe first, then ask; keep the answer's
 * revision, drop an event at or below it, apply one at exactly the next, and on a gap (an event
 * further ahead) ask again. An event that lands while an ask is in flight waits for its answer;
 * one that lands with no answer and no ask in flight is dropped, since the next answer holds it.
 * A failed ask is retried with backoff. Returns the unsubscribe, whether or not the first ask
 * answered.
 */
export async function followMenuBar(onMenus: (menus: BarMenuView[]) => void): Promise<UnlistenFn> {
  let menus: BarMenuView[] | null = null
  let revision: number | null = null
  let buffered: MenuBarChanged[] = []
  let asking = false
  let stopped = false
  let retryMs = FIRST_RETRY_MS
  let retryTimer: ReturnType<typeof setTimeout> | null = null

  function apply(change: MenuBarChanged): void {
    if (menus === null || revision === null) {
      if (asking) buffered.push(change)
      return
    }
    if (change.revision <= revision) return
    if (change.revision !== revision + 1) {
      void ask()
      return
    }
    menus = applyMenuBarChange(menus, change)
    revision = change.revision
    onMenus(menus)
  }

  function retryLater(): void {
    if (stopped || retryTimer !== null) return
    retryTimer = setTimeout(() => {
      retryTimer = null
      void ask()
    }, retryMs)
    retryMs = Math.min(retryMs * 2, MAX_RETRY_MS)
  }

  async function ask(): Promise<void> {
    if (asking) return
    asking = true
    // Everything from here on waits for the answer, which already holds anything older.
    menus = null
    revision = null
    buffered = []
    let answered = false
    try {
      const answer = await commands.getMenuBar()
      if (stopped) return
      menus = answer.menus
      revision = answer.revision
      answered = true
      retryMs = FIRST_RETRY_MS
      onMenus(menus)
    } catch (error) {
      log.warn("Couldn't get the menu bar, retrying: {error}", { error })
    } finally {
      asking = false
    }
    const waiting = buffered
    buffered = []
    if (!answered) {
      retryLater()
      return
    }
    for (const change of waiting) apply(change)
  }

  const unlisten = await events.menuBarChanged.listen((event) => {
    apply(event.payload)
  })
  await ask()
  return () => {
    stopped = true
    if (retryTimer !== null) clearTimeout(retryTimer)
    unlisten()
  }
}
