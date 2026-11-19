/**
 * Where each desktop's menus live, with "Show menu bar" on and off, and the main-menu button's
 * rows (task 63 step 8 of `docs/specs/windows-port-plan.md`; `menu-model.md`'s per-desktop table).
 */
import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'
import type { BarMenuView, Row } from '#lib/ipc/bindings.js'
import { CONVENTIONS_ON, type ConventionsDesktop } from '#lib/platform/conventions.js'
import {
  drawsBar,
  menuBarShown,
  menuLayout,
  PRIMARY_MENU_IDS,
  primaryMenuRows,
  showsMainMenuButton,
} from './menu-layout'

describe('menuBarShown', () => {
  it('follows the desktop until the user flips it: the bar everywhere but GNOME', () => {
    const shown = Object.fromEntries(
      (Object.keys(CONVENTIONS_ON) as ConventionsDesktop[]).map((desktop) => [
        desktop,
        menuBarShown('desktopDefault', CONVENTIONS_ON[desktop]),
      ]),
    )
    expect(shown).toEqual({ macos: true, windows: true, gnome: false, kde: true, otherLinux: true })
  })

  it('keeps what the user chose on every desktop', () => {
    for (const desktop of Object.keys(CONVENTIONS_ON) as ConventionsDesktop[]) {
      expect(menuBarShown('shown', CONVENTIONS_ON[desktop]), desktop).toBe(true)
      expect(menuBarShown('hidden', CONVENTIONS_ON[desktop]), desktop).toBe(false)
    }
  })
})

describe('menuLayout', () => {
  it('starts each desktop the way menu-model.md says', () => {
    const layouts = Object.fromEntries(
      (Object.keys(CONVENTIONS_ON) as ConventionsDesktop[]).map((desktop) => [
        desktop,
        menuLayout(CONVENTIONS_ON[desktop], menuBarShown('desktopDefault', CONVENTIONS_ON[desktop])),
      ]),
    )
    expect(layouts).toEqual({ macos: 'systemBar', windows: 'bar', gnome: 'mainMenu', kde: 'bar', otherLinux: 'bar' })
  })

  it('shows the bar everywhere off macOS with the setting on, GNOME included', () => {
    for (const desktop of ['windows', 'gnome', 'kde', 'otherLinux'] as const) {
      expect(menuLayout(CONVENTIONS_ON[desktop], true), desktop).toBe('bar')
    }
  })

  it('hides it behind Alt on Windows and behind the main-menu button on Linux', () => {
    expect(menuLayout(CONVENTIONS_ON.windows, false)).toBe('barOnAlt')
    for (const desktop of ['gnome', 'kde', 'otherLinux'] as const) {
      expect(menuLayout(CONVENTIONS_ON[desktop], false), desktop).toBe('mainMenu')
    }
  })

  it('leaves macOS to its system bar whatever the setting says', () => {
    expect(menuLayout(CONVENTIONS_ON.macos, false)).toBe('systemBar')
  })

  it('draws the bar and the button in exactly the layouts that need them', () => {
    expect([drawsBar('systemBar'), drawsBar('bar'), drawsBar('barOnAlt'), drawsBar('mainMenu')]).toEqual([
      false,
      true,
      true,
      false,
    ])
    expect(
      [showsMainMenuButton('systemBar'), showsMainMenuButton('bar'), showsMainMenuButton('barOnAlt')].concat(
        showsMainMenuButton('mainMenu'),
      ),
    ).toEqual([false, false, true, true])
  })
})

const item = (id: string): Row => ({
  kind: 'item',
  id,
  label: { key: `menu.${id}`, args: {} },
  shortcut: null,
  ariaKeyshortcuts: null,
  enabled: true,
})

const MENUS: BarMenuView[] = [
  {
    id: 'menu_file',
    title: { key: 'menu.bar.file', args: {} },
    rows: [item('open'), { kind: 'separator' }, item('rename'), { kind: 'separator' }, item('quit')],
  },
  { id: 'menu_edit', title: { key: 'menu.bar.edit', args: {} }, rows: [item('edit_copy'), item('settings')] },
  {
    id: 'menu_help',
    title: { key: 'menu.bar.help', args: {} },
    rows: [item('about'), { kind: 'separator' }, item('help_shortcuts')],
  },
]

function ids(rows: readonly Row[]): string[] {
  return rows.map((row) => (row.kind === 'separator' ? '---' : 'id' in row ? row.id : row.kind))
}

describe('primaryMenuRows', () => {
  it('holds every bar menu as a submenu, rows untouched, without the standard group', () => {
    const rows = primaryMenuRows(MENUS, { standardGroup: false })
    expect(ids(rows)).toEqual(['menu_file', 'menu_edit', 'menu_help'])
    expect(rows[0]).toMatchObject({ kind: 'submenu', label: MENUS[0].title, rows: MENUS[0].rows, enabled: true })
  })

  it('ends with GNOME’s standard group and drops Quit from File with it', () => {
    const rows = primaryMenuRows(MENUS, { standardGroup: true })
    expect(ids(rows)).toEqual(['menu_file', 'menu_edit', 'menu_help', '---', 'settings', 'help_shortcuts', 'about'])
    const file = rows[0]
    expect(file.kind === 'submenu' ? ids(file.rows) : []).toEqual(['open', '---', 'rename'])
  })

  it('names the ids cmdr-menu builds', () => {
    // here = apps/desktop/src/lib/ui/command-menu, so the repo root is six up.
    const here = path.dirname(fileURLToPath(import.meta.url))
    const source = readFileSync(path.resolve(here, '../../../../../../crates/cmdr-menu/src/ids.rs'), 'utf8')
    const constants: Record<keyof typeof PRIMARY_MENU_IDS, string> = {
      quit: 'QUIT_ID',
      settings: 'SETTINGS_ID',
      keyboardShortcuts: 'HELP_SHORTCUTS_ID',
      about: 'ABOUT_ID',
    }
    for (const [key, constant] of Object.entries(constants)) {
      expect(source, constant).toContain(
        `pub const ${constant}: &str = "${PRIMARY_MENU_IDS[key as keyof typeof PRIMARY_MENU_IDS]}";`,
      )
    }
  })
})
