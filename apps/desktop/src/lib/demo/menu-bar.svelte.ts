/**
 * The demo's top menu bar: the visitor's own OS's (D5), above the app. A Mac visitor gets the
 * macOS bar a browser doesn't have, everyone else their OS's in-window bar, both on the app's own
 * in-page bar (`#lib/ui/MenuBar.svelte`) in their OS's flavor and look. It draws the desktop's own
 * bar for that OS (`menu-engine.ts`, over `crates/cmdr-menu`) and sends exactly what a native click
 * sends, so every handler in the app is the one the native menu reaches. This file only mounts it
 * and feeds it: which rows exist, what they say, their shortcuts, whether they're enabled or
 * checked, and what a click sends all come from the engine, and a pick is `menu-click.ts`'s. See
 * `DETAILS.md` § The menu bar.
 *
 * Keys are the component's. On a Mac, ⌃F2 focuses the bar (macOS's own "Move focus to the menu
 * bar"); elsewhere a lone Alt or F10 does, and Alt plus a title's letter opens it. Then ←/→ move
 * between menus, ↓ / Enter / Space open one, ↑/↓ walk its enabled rows, → opens a submenu, ← closes
 * it, Enter / Space pick, and Esc backs out one level.
 */
import { mount, unmount } from 'svelte'
import MenuBar from '#lib/ui/MenuBar.svelte'
import { getMessage } from '#lib/intl/messages.svelte.js'
import type { BarMenu, MenuEngineHandle } from './menu-engine.js'
import { demoMenuClass, ensureMenuStyle } from './menu-bar-style.js'
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
  // Where the main window starts under the bar, for the demo's other chrome that sits on the window:
  // the look's own bar height (30 px on a Mac, 40 on Windows), so the two never disagree.
  document.documentElement.style.setProperty('--cmdr-demo-window-top', 'var(--spacing-menu-bar-height)')

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
      // No `flavor`: the component takes the visitor's from their menu model (`platform.ts`), so a
      // Mac visitor walks a Mac bar and everyone else their OS's in-window bar (task 63 step 11).
      class: demoMenuClass(),
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
