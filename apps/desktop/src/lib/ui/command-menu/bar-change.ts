/**
 * One `menu-bar-changed` applied to the bar a page holds: the whole bar when its shape moved,
 * else each moved row by id, submenus included. Pure; `#lib/tauri-commands`' `followMenuBar`
 * runs the ordering contract around it.
 */

import type { BarMenuView, MenuBarChanged, Row, RowState } from '#lib/ipc/bindings.js'

function applyRowStates(rows: readonly Row[], states: ReadonlyMap<string, RowState>): Row[] {
  return rows.map((row): Row => {
    if (row.kind === 'item' || row.kind === 'check') {
      const state = states.get(row.id)
      if (!state) return row
      const moved = {
        label: state.label,
        shortcut: state.shortcut,
        ariaKeyshortcuts: state.ariaKeyshortcuts,
        enabled: state.enabled,
      }
      return row.kind === 'check' ? { ...row, ...moved, checked: state.checked ?? row.checked } : { ...row, ...moved }
    }
    if (row.kind === 'submenu') {
      const state = states.get(row.id)
      const moved = state ? { label: state.label, enabled: state.enabled } : {}
      return { ...row, ...moved, rows: applyRowStates(row.rows, states) }
    }
    return row
  })
}

export function applyMenuBarChange(menus: readonly BarMenuView[], change: MenuBarChanged): BarMenuView[] {
  if (change.menus) return change.menus
  const states = new Map(change.rows.map((state) => [state.id, state]))
  return menus.map((menu) => ({ ...menu, rows: applyRowStates(menu.rows, states) }))
}
