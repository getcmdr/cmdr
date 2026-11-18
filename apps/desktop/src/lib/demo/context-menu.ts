/**
 * The demo's right-click menus, in place of the native popups a browser doesn't have: when the
 * app sends a `show_*_context_menu` command, the backend asks the engine for the menu's rows
 * (`crates/cmdr-listing-wasm/src/context_menu.rs`, over `crates/cmdr-menu`'s `context`) and this
 * draws them with the app's own in-page context menu (`#lib/ui/ContextMenu.svelte`, the one
 * Windows draws), in the visitor's OS's rows and look (macOS 27's for a Mac visitor, Windows 11's
 * for a Windows one), in the top frame (so a menu can reach past a panel's edge, as a native one
 * passes a window's). A pick sends what the native item sends (`menu-click.ts`). No menu logic
 * here: rows, words, shortcuts, enabled state, and what a pick does are the engine's.
 *
 * Keys are the component's: ↑/↓ walk the enabled rows, Home/End jump, → opens a submenu, ← or Esc
 * closes it, Esc closes the menu, Enter / Space pick, Tab closes; off the Mac a row's underlined
 * letter picks it too. A menu opened from the keyboard
 * (the app sends an anchor) starts on its first enabled row. Every key goes to the open menu,
 * never the app behind it.
 */
import { mount, unmount } from 'svelte'
import ContextMenu from '#lib/ui/ContextMenu.svelte'
import { createContextMenu } from '#lib/ui/command-menu/context-menu.svelte.js'
import type { ContextPopup, MenuEngineHandle } from './menu-engine.js'
import { type MenuClickDeps, performMenuClick } from './menu-click.js'
import { demoMenuClass, ensureMenuStyle } from './menu-bar-style.js'
import { convention } from '#lib/platform/conventions.js'

export interface ContextMenuDeps extends MenuClickDeps {
  menu: Pick<MenuEngineHandle, 'contextClick'>
}

export interface ContextMenuHost {
  /**
   * Opens `popup` at `at`, in the top frame's viewport pixels. `fromKeyboard` starts on the first
   * enabled row, as a menu opened by a key does.
   */
  open(popup: ContextPopup, at: { x: number; y: number }, fromKeyboard: boolean): void
  /** Closes the open menu, if any. */
  close(): void
  /** Takes the host down (a test's teardown). */
  destroy(): void
}

export function createContextMenuHost(deps: ContextMenuDeps): ContextMenuHost {
  const dropStyle = ensureMenuStyle()
  const menu = createContextMenu({
    onPick: (menuId) => {
      performMenuClick(deps.menu.contextClick(menuId), deps)
    },
    // The visitor's OS's, as the bar's: a Mac menu for a Mac visitor, Windows' in-page one elsewhere.
    flavor: () => (convention('menuModel') === 'systemMenuBar' ? 'mac' : 'desktop'),
  })
  const host = document.createElement('div')
  host.style.display = 'contents'
  document.body.append(host)
  const component = mount(ContextMenu, { target: host, props: { menu, class: demoMenuClass() } })

  return {
    open(popup, at, fromKeyboard) {
      menu.open({ rows: popup.rows, at, fromKeyboard })
    },
    close: () => {
      menu.close()
    },
    destroy: () => {
      void unmount(component)
      host.remove()
      dropStyle()
    },
  }
}
