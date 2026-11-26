/**
 * The trailing action row: one last row a pane's KIND may ask its list to draw after every
 * entry, the `..` row's mirror image (the Servers list's "Add server…"). The pane fills it from
 * its kind (`../pane/list-kind-words.ts::trailingRowFor`); both views take it as a plain prop,
 * so ❌ no view ever asks which kind it shows.
 *
 * ❗ It's no entry. The listing engine never sees it, so sort, the quick filter, and
 * type-to-jump can't move, hide, or match it, by construction. It's never selected, counted,
 * dragged, dropped on, or an operation's source or target. The cursor reaches it, and opening
 * it runs its command.
 *
 * Its UI index is the entry rows' count (`..` included): `parentOffset + rowCount`. Every
 * consumer that spans the cursor's rows (key ranges, the virtual scroller, Brief's columns, row
 * positions in the listbox, the MCP mirror) reads the counts from here, ❌ never its own `+ 1`.
 */

import type { CommandId } from '#lib/commands/index.js'

export interface TrailingRow {
  /** What the row says, which is also its accessible name ("Add server…"). */
  label: string
  /** What Enter, a double-click, and MCP `open_under_cursor` on it run. */
  commandId: CommandId
  /**
   * The locale-independent name `cmdr://state` lists it under, the one MCP `move_cursor` finds it
   * by (the row's words are translated; an agent's spelling mustn't be).
   */
  mcpName: string
}

/** How many trailing rows a list draws: one when its kind asks for one. */
export function trailingRowCount(row: TrailingRow | undefined): 0 | 1 {
  return row ? 1 : 0
}

/**
 * The rows the cursor and the views span: the entry rows (`..` included, the pane's
 * `effectiveTotalCount` and the views' `totalCount`) plus the trailing row.
 */
export function cursorRowCount(entryRows: number, row: TrailingRow | undefined): number {
  return entryRows + trailingRowCount(row)
}

/** The trailing row's UI index (right after the last entry), or `null` when there's none. */
export function trailingRowIndex(entryRows: number, row: TrailingRow | undefined): number | null {
  return row ? entryRows : null
}

/** The trailing row's index when it falls in `[start, end)` (a view's window or column), else `null`. */
export function trailingRowWithin(trailingIndex: number | null, start: number, end: number): number | null {
  return trailingIndex !== null && trailingIndex >= start && trailingIndex < end ? trailingIndex : null
}

/** Whether `index` is the trailing row. */
export function isTrailingRowIndex(index: number, entryRows: number, row: TrailingRow | undefined): boolean {
  return row !== undefined && index === entryRows
}

/**
 * Where the cursor goes when the entry rows change under it (a re-sort, a hidden-files toggle,
 * a filter) and nothing followed its row by name: a cursor that sat ON the trailing row stays on
 * it, anything else clamps to the last entry, ❌ never onto the trailing row (a row that
 * vanished isn't replaced by an action).
 */
export function clampCursorAcrossRebuild(input: {
  cursorIndex: number
  /** The entry rows before the change. */
  previousEntryRows: number
  /** The entry rows after it. */
  entryRows: number
  row: TrailingRow | undefined
}): number {
  const { cursorIndex, previousEntryRows, entryRows, row } = input
  if (isTrailingRowIndex(cursorIndex, previousEntryRows, row)) return entryRows
  // An empty list leaves index 0: the trailing row when there is one, else nothing.
  return Math.max(0, Math.min(cursorIndex, entryRows - 1))
}

/**
 * Brief's column widths with the trailing row's column at least `minWidth` wide. The measured
 * widths are the widest ENTRY name per column (the backend never sees the trailing row), so a
 * column of short names would clip the row's words ("Add server…" under three-letter servers).
 * `minWidth` is the width an unmeasured column renders at, which fits a short action label.
 */
export function widenTrailingColumn(input: {
  widths: readonly number[]
  trailingIndex: number | null
  itemsPerColumn: number
  minWidth: number
}): number[] {
  const { widths, trailingIndex, itemsPerColumn, minWidth } = input
  const widened = [...widths]
  if (trailingIndex === null) return widened
  const column = Math.floor(trailingIndex / Math.max(1, itemsPerColumn))
  if (column < widened.length) widened[column] = Math.max(widened[column], minWidth)
  return widened
}
