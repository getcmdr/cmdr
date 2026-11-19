/**
 * Where a window's menus live, from its desktop's conventions and the "Show menu bar" setting
 * (D5, task 63 of `docs/specs/windows-port-plan.md`; per desktop: `docs/notes/windows-port/menu-model.md`).
 * Pure, so the main window, the viewer, and their tests ask one rule.
 */
import type { BarMenuView, PlatformConventions, Row } from '#lib/ipc/bindings.js'

/**
 * - `systemBar`: the OS owns the bar (macOS); the page draws none.
 * - `bar`: the in-window bar, always showing.
 * - `barOnAlt`: the bar hidden, shown by a tap of Alt or F10 until focus leaves it (Windows), with
 *   the main-menu button standing in.
 * - `mainMenu`: no bar; the main-menu button holds the menus and F10 opens it (GNOME's default,
 *   and KDE's or another Linux desktop's hidden bar).
 */
export type MenuLayout = 'systemBar' | 'bar' | 'barOnAlt' | 'mainMenu'

/**
 * The "Show menu bar" setting as stored (`appearance.menuBar`): `desktopDefault` until the user
 * flips it, so each desktop starts the way its own apps do. A literal default rather than a
 * per-desktop one, because the analytics defaults manifest resolves an absent key against one
 * value per version (`apps/desktop/scripts/gen-analytics-defaults-lib.ts`).
 */
export type MenuBarChoice = 'desktopDefault' | 'shown' | 'hidden'

/**
 * Whether the bar shows: the user's choice, or the desktop's default, which is the bar everywhere
 * but GNOME (its HIG has a main menu button instead, `menu-model.md`).
 */
export function menuBarShown(choice: MenuBarChoice, conventions: Pick<PlatformConventions, 'menuModel'>): boolean {
  if (choice === 'desktopDefault') return conventions.menuModel !== 'primaryMenu'
  return choice === 'shown'
}

/** The layout for a window on a desktop with `conventions`, with the "Show menu bar" setting at `showMenuBar`. */
export function menuLayout(
  conventions: Pick<PlatformConventions, 'menuModel' | 'hiddenMenuBar'>,
  showMenuBar: boolean,
): MenuLayout {
  if (conventions.menuModel === 'systemMenuBar') return 'systemBar'
  if (showMenuBar) return 'bar'
  return conventions.hiddenMenuBar === 'showsOnAltOrF10' ? 'barOnAlt' : 'mainMenu'
}

/** Whether `layout` draws the bar element (shown, or waiting for Alt). */
export function drawsBar(layout: MenuLayout): boolean {
  return layout === 'bar' || layout === 'barOnAlt'
}

/** Whether `layout` shows the main-menu button. */
export function showsMainMenuButton(layout: MenuLayout): boolean {
  return layout === 'barOnAlt' || layout === 'mainMenu'
}

/**
 * The bar's ids the main menu leans on, from `crates/cmdr-menu/src/ids.rs` (`primary-menu.test.ts`
 * reads that file, so a renamed id fails there).
 */
export const PRIMARY_MENU_IDS = {
  quit: 'quit',
  settings: 'settings',
  keyboardShortcuts: 'help_shortcuts',
  about: 'about',
} as const

/**
 * The main-menu button's rows: every menu of the bar as a submenu, in the bar's order. With
 * `standardGroup` (GNOME's `primaryMenu` model) the menu ends the way GNOME's HIG asks: its
 * standard group (Settings, Keyboard shortcuts, About Cmdr, copied from the bar's own rows, so a
 * pick runs the same id) and no Quit in File. The rows stay the backend's: this only arranges them.
 */
export function primaryMenuRows(menus: readonly BarMenuView[], options: { standardGroup: boolean }): Row[] {
  const rows: Row[] = menus.map((menu) => ({
    kind: 'submenu',
    id: menu.id,
    label: menu.title,
    enabled: true,
    rows: options.standardGroup ? withoutRow(menu.rows, PRIMARY_MENU_IDS.quit) : menu.rows,
  }))
  if (!options.standardGroup) return rows
  const group = [PRIMARY_MENU_IDS.settings, PRIMARY_MENU_IDS.keyboardShortcuts, PRIMARY_MENU_IDS.about]
    .map((id) => findRow(menus, id))
    .filter((row): row is Row => row !== null)
  return group.length > 0 ? [...rows, { kind: 'separator' }, ...group] : rows
}

/** `rows` without the row `id`, and without the separator that would be left dangling or doubled. */
function withoutRow(rows: readonly Row[], id: string): Row[] {
  const kept = rows.filter((row) => !('id' in row) || row.id !== id)
  return kept.filter(
    (row, index) =>
      row.kind !== 'separator' || (index > 0 && index < kept.length - 1 && kept[index + 1].kind !== 'separator'),
  )
}

function findRow(menus: readonly BarMenuView[], id: string): Row | null {
  const walk = (rows: readonly Row[]): Row | null => {
    for (const row of rows) {
      if ('id' in row && row.id === id && row.kind !== 'submenu') return row
      if (row.kind === 'submenu') {
        const found = walk(row.rows)
        if (found) return found
      }
    }
    return null
  }
  for (const menu of menus) {
    const found = walk(menu.rows)
    if (found) return found
  }
  return null
}
