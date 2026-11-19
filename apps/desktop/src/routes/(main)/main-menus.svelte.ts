/**
 * The main window's menus where the page draws them (`pageDrawsMenuBar()`: off macOS; D5, task 63
 * of `docs/specs/windows-port-plan.md`): the backend's bar (`get_menu_bar`, followed live by
 * `followMenuBar`), shared by the bar (`MainMenuBar.svelte`) and the main-menu button in the tab
 * row, so both draw the same rows. A pick from either goes back through `pick_menu_bar_item`, the
 * handler a native click reaches.
 */
import type { BarMenuView, BarPickVia } from '#lib/ipc/bindings.js'
import { followMenuBar, pickMenuBarItem } from '#lib/tauri-commands/index.js'
import { getAppLogger } from '#lib/logging/logger.js'

const log = getAppLogger('inPageMenus')

export interface MainMenus {
  /** The bar's menus as last answered; empty until the first answer, and in the browser demo. */
  readonly menus: BarMenuView[]
  /** Starts following the bar; returns the teardown. */
  start(): () => void
}

export function createMainMenus(): MainMenus {
  let menus = $state<BarMenuView[]>([])
  return {
    get menus() {
      return menus
    },
    start() {
      let stop: (() => void) | undefined
      let stopped = false
      void followMenuBar((next) => {
        menus = next
      }).then((unlisten) => {
        if (stopped) unlisten()
        else stop = unlisten
      })
      return () => {
        stopped = true
        stop?.()
      }
    },
  }
}

/**
 * Runs a row the bar or the main-menu button picked, as a native click would. `via` says whether a
 * pointer click or a key picked it, which the text-field gate needs (`text-field-gate.ts`).
 */
export function pickMainMenuRow(menuId: string, via: BarPickVia): void {
  pickMenuBarItem(menuId, via).catch((error: unknown) => {
    log.warn("Couldn't run a menu bar pick: {error}", { error })
  })
}
