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
import { applyMenuBarChange } from '#lib/ui/command-menu/bar-change.js'

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

/**
 * Follows the main window's bar: calls `onMenus` with the whole bar now and after every change.
 * The ordering contract (`menu/DETAILS.md`): subscribe first, then ask; keep the answer's
 * revision, drop an event at or below it, apply one at exactly the next, and on a gap (an event
 * further ahead) ask again. An event that lands before the answer waits for it. Returns the
 * unsubscribe.
 */
export async function followMenuBar(onMenus: (menus: BarMenuView[]) => void): Promise<UnlistenFn> {
  let menus: BarMenuView[] | null = null
  let revision: number | null = null
  let buffered: MenuBarChanged[] = []
  let asking = false
  let stopped = false

  function apply(change: MenuBarChanged): void {
    if (menus === null || revision === null) {
      buffered.push(change)
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

  async function ask(): Promise<void> {
    if (asking) return
    asking = true
    // Everything from here on waits for the answer, which already holds anything older.
    menus = null
    revision = null
    try {
      const answer = await commands.getMenuBar()
      if (stopped) return
      menus = answer.menus
      revision = answer.revision
      onMenus(menus)
    } finally {
      asking = false
    }
    const waiting = buffered
    buffered = []
    for (const change of waiting) apply(change)
  }

  const unlisten = await events.menuBarChanged.listen((event) => {
    apply(event.payload)
  })
  await ask()
  return () => {
    stopped = true
    unlisten()
  }
}
