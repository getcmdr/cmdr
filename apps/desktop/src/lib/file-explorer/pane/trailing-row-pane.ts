/**
 * The pane's side of a kind's trailing action row (`../views/trailing-row.ts`): the views draw
 * it, and this answers everything the pane asks about it, so `FilePane` reads ONE place:
 *
 * - how many rows the cursor spans (the arrow, Page, Home, and End keys, MCP `move_cursor`),
 * - where selection stops (`selection-state.svelte.ts`'s `getSelectableEnd`),
 * - whether the cursor is on it, and running its command (Enter, a double-click, MCP
 *   `open_under_cursor`), through the pane's `onCommand`, the F-key bar's own road,
 * - where the cursor goes when the entries change under it,
 * - its line in the MCP mirror, and the name `move_cursor` finds it by.
 *
 * ❌ No kind check here: the row comes from `list-kind-words.ts::trailingRowFor`.
 */

import type { CommandId } from '#lib/commands/index.js'
import type { PaneFileEntry } from '#lib/tauri-commands/index.js'
import {
  clampCursorAcrossRebuild,
  cursorRowCount,
  isTrailingRowIndex,
  trailingRowIndex,
  type TrailingRow,
} from '../views/trailing-row'

export interface TrailingRowPaneDeps {
  /** The pane kind's trailing row, or `undefined` for none. */
  getRow: () => TrailingRow | undefined
  /** The entry rows, `..` included: `FilePane`'s `effectiveTotalCount`. */
  getEntryRows: () => number
  getCursorIndex: () => number
  /** Runs a command the way the F-key bar does (`FilePane`'s `onCommand`). */
  runCommand: (commandId: CommandId) => void
}

export interface TrailingRowPane {
  /** Every row the cursor reaches: the entries plus the trailing row. */
  cursorRows: () => number
  /** The first index no gesture may select (`Infinity` with no trailing row). */
  selectableEnd: () => number
  isUnderCursor: () => boolean
  /** Runs the row's command (a double-click on it). */
  run: () => void
  /** Runs it when the cursor is on it (Enter, MCP `open_under_cursor`); false when it isn't. */
  activateUnderCursor: () => boolean
  /** Its index when `name` is its MCP name or its label, else `null` (MCP `move_cursor` by name). */
  indexNamed: (name: string) => number | null
  /** Where the cursor goes when the entries change and no row followed it by name. */
  cursorAcrossRebuild: (args: { cursorIndex: number; previousEntryRows: number }) => number
  /** Its MCP mirror line, when the mirrored window (`visibleEnd`, exclusive) reaches it. */
  mcpEntry: (visibleEnd: number) => PaneFileEntry | null
}

export function createTrailingRowPane(deps: TrailingRowPaneDeps): TrailingRowPane {
  const isUnderCursor = () => isTrailingRowIndex(deps.getCursorIndex(), deps.getEntryRows(), deps.getRow())
  const run = () => {
    const row = deps.getRow()
    if (row) deps.runCommand(row.commandId)
  }
  return {
    cursorRows: () => cursorRowCount(deps.getEntryRows(), deps.getRow()),
    selectableEnd: () => (deps.getRow() ? deps.getEntryRows() : Number.POSITIVE_INFINITY),
    isUnderCursor,
    run,
    activateUnderCursor() {
      if (!isUnderCursor()) return false
      run()
      return true
    },
    indexNamed(name) {
      const row = deps.getRow()
      if (!row || (name !== row.mcpName && name !== row.label)) return null
      return trailingRowIndex(deps.getEntryRows(), row)
    },
    cursorAcrossRebuild: ({ cursorIndex, previousEntryRows }) =>
      clampCursorAcrossRebuild({ cursorIndex, previousEntryRows, entryRows: deps.getEntryRows(), row: deps.getRow() }),
    mcpEntry(visibleEnd) {
      const row = deps.getRow()
      const index = trailingRowIndex(deps.getEntryRows(), row)
      if (!row || index === null || visibleEnd <= index) return null
      // No path: it's no item, and `action` keeps every MCP file tool off it (`PaneFileEntry::is_action`).
      return {
        name: row.mcpName,
        path: '',
        isDirectory: false,
        size: null,
        recursiveSize: null,
        modified: null,
        recursiveSizeUpdating: null,
        action: row.commandId,
      }
    },
  }
}
