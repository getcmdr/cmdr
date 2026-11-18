/**
 * The demo's top menu bar, in place of the macOS one a browser doesn't have: the app's own
 * in-page bar (`#lib/ui/MenuBar.svelte`, the one Windows and Linux draw) on its Mac flavor and
 * look, above the app. It draws the desktop's own bar (`menu-engine.ts`, over `crates/cmdr-menu`)
 * and sends exactly what a native click sends, so every handler in the app is the one the native
 * menu reaches. This file only mounts it and feeds it: which rows exist, what they say, their
 * shortcuts, whether they're enabled or checked, and what a click sends all come from the engine,
 * and a pick is `menu-click.ts`'s. See `DETAILS.md` § The menu bar.
 *
 * Keys are the component's Mac flavor: ⌃F2 focuses the bar (macOS's own "Move focus to the menu
 * bar"), ←/→ move between menus, ↓ / Enter / Space open one, ↑/↓ walk its enabled rows, → opens a
 * submenu, ← closes it, Enter / Space pick, and Esc backs out one level.
 */
import { mount, unmount } from 'svelte'
import MenuBar from '#lib/ui/MenuBar.svelte'
import { getMessage } from '#lib/intl/messages.svelte.js'
import type { BarMenu, MenuEngineHandle } from './menu-engine.js'
import { DEMO_MENU_CLASS, MENU_BAR_HEIGHT_PX, ensureMenuStyle } from './menu-bar-style.js'
import { type MenuClickDeps, performMenuClick } from './menu-click.js'

export interface MenuBarDeps extends MenuClickDeps {
  menu: Pick<MenuEngineHandle, 'bar' | 'click' | 'ready' | 'onChange'>
}

export interface MenuBarHandle {
  /** The bar's element, first in `<body>`. */
  readonly element: HTMLElement
  /** Takes the bar down (a test's teardown). */
  destroy(): void
}

export function installMenuBar(deps: MenuBarDeps): MenuBarHandle {
  const dropStyle = ensureMenuStyle()
  const host = document.createElement('div')
  host.className = 'cmdr-demo-menubar-host'
  host.style.display = 'contents'
  document.body.prepend(host)
  // Where the main window starts under the bar, for the demo's other chrome that sits on the window.
  document.documentElement.style.setProperty('--cmdr-demo-window-top', `${String(MENU_BAR_HEIGHT_PX)}px`)

  // The engine's bar as it stands: redrawn after every push and click, the demo's `menu-bar-changed`.
  const state = $state<{ menus: BarMenu[] }>({ menus: [] })
  const refresh = (): void => {
    const fresh = deps.menu.bar()
    if (fresh) state.menus = fresh
  }
  const stopFollowing = deps.menu.onChange(refresh)

  const component = mount(MenuBar, {
    target: host,
    props: {
      get menus() {
        return state.menus
      },
      onPick: (menuId: string) => {
        performMenuClick(deps.menu.click(menuId), deps)
      },
      // Read in the markup, so the name follows `setLocale()` with the rest of the UI.
      get ariaLabel() {
        return getMessage('ui.menuBar.label')
      },
      // The macOS bar for every visitor, until the demo follows the visitor's OS (task 63 step 11).
      flavor: 'mac',
      class: DEMO_MENU_CLASS,
    },
  })

  let destroyed = false
  void deps.menu.ready.then(() => {
    // Drawn the moment the engine is up.
    if (!destroyed) refresh()
  })

  return {
    get element() {
      return host.querySelector<HTMLElement>('[role="menubar"]') ?? host
    },
    destroy: () => {
      destroyed = true
      stopFollowing()
      void unmount(component)
      host.remove()
      dropStyle()
      document.documentElement.style.removeProperty('--cmdr-demo-window-top')
    },
  }
}
