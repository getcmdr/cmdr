/**
 * The demo's menu bar state: `DemoMenuBar` from the WASM engine (`crates/cmdr-listing-wasm/src/menu.rs`
 * over `crates/cmdr-menu`). It answers the menu-state pushes the app sends the native menu
 * (`activate_window_menu`, `update_menu_accelerator`, `set_commands_refused_over_dialog`, …) and
 * draws the macOS bar from them, so this file holds no menu logic: it loads, routes, and types the
 * answers. See `DETAILS.md` § The menu bar.
 */
import type { BarMenuView, Row } from '#lib/ipc/bindings.js'
import type { Payload } from './answer-types.js'
import { loadEngineModule } from './engine-module.js'

/**
 * One row of a drawn menu: `cmdr-menu`'s page `Row`, the one the desktop answers too
 * (`get_menu_bar`), so the shared menu component draws both.
 */
export type MenuRow = Row

/** One top-level menu of the drawn bar. */
export type BarMenu = BarMenuView

/** What a click asks the page to do, as `menu.rs`'s `Click` serializes. */
export type MenuClick =
  | { kind: 'emit'; event: string; payload: unknown; to: string }
  | { kind: 'invoke'; command: string; args: Payload }
  | { kind: 'nativeEdit'; action: 'cut' | 'copy' | 'paste' | 'selectAll' }
  | { kind: 'closeFocusedWindow' }
  | { kind: 'nothing' }

/** A right-click menu as `context_menu.rs`'s `Popup` serializes: its rows, and the anchor in the calling frame's pixels. */
export interface ContextPopup {
  rows: MenuRow[]
  anchor: { x: number; y: number } | null
}

export interface MenuEngineHandle {
  /** Whether `invoke` answers `cmd`. Waits for the engine, so an early push isn't lost. */
  handles(cmd: string): Promise<boolean>
  /** Stores one push, as `invoke` would answer it. */
  invoke(cmd: string, payload: Payload): Promise<unknown>
  /** The bar as it stands, or `null` before (or without) the engine. */
  bar(): BarMenu[] | null
  /** What a click on `menuId` sends. */
  click(menuId: string): MenuClick
  /** Whether `popup` answers `cmd` (a `show_*` context-menu command). Waits for the engine. */
  opensPopup(cmd: string): Promise<boolean>
  /** Answers a `show_*` command opened over the window `label`: the rows to draw, and where. */
  popup(cmd: string, payload: Payload, label: string): Promise<ContextPopup>
  /** What a pick of `menuId` from the last popup sends. */
  contextClick(menuId: string): MenuClick
  /** Resolves once the engine answered, with whether it did. */
  readonly ready: Promise<boolean>
  /**
   * Calls `listener` after every push and bar click, the moments the drawn bar can move (the
   * demo's `menu-bar-changed`); returns the unsubscribe.
   */
  onChange(listener: () => void): () => void
}

export function startMenuEngine(): MenuEngineHandle {
  let menu: InstanceType<typeof import('virtual:cmdr-listing-wasm').DemoMenuBar> | null = null
  let DemoMenuBar: typeof import('virtual:cmdr-listing-wasm').DemoMenuBar | null = null
  const ready = loadEngineModule().then((wasm) => {
    if (!wasm) return false
    DemoMenuBar = wasm.DemoMenuBar
    menu = new wasm.DemoMenuBar()
    return true
  })

  const listeners = new Set<() => void>()
  const changed = (): void => {
    for (const listener of listeners) listener()
  }

  return {
    ready,
    handles: async (cmd) => (await ready) && DemoMenuBar !== null && DemoMenuBar.handles(cmd),
    invoke: async (cmd, payload) => {
      await ready
      if (!menu) throw new Error(`[demo] the menu bar did not load, so it can't answer ${cmd}`)
      const answer: unknown = menu.invoke(cmd, payload)
      changed()
      return answer
    },
    bar: () => (menu ? (menu.bar() as BarMenu[]) : null),
    click: (menuId) => {
      const outcome = menu ? (menu.click(menuId) as MenuClick) : ({ kind: 'nothing' } as const)
      // A check item flips itself on a click, as the native one does.
      changed()
      return outcome
    },
    onChange: (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    opensPopup: async (cmd) => (await ready) && DemoMenuBar !== null && DemoMenuBar.opensPopup(cmd),
    popup: async (cmd, payload, label) => {
      await ready
      if (!menu) throw new Error(`[demo] the menus did not load, so they can't open ${cmd}`)
      return menu.popup(cmd, payload, label) as ContextPopup
    },
    contextClick: (menuId) => (menu ? (menu.contextClick(menuId) as MenuClick) : { kind: 'nothing' }),
  }
}
