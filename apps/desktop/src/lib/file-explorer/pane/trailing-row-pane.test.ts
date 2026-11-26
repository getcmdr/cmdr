/**
 * The pane's answers about a kind's trailing action row, on a stub kind: `..` at 0, three
 * entries, the trailing row at 4.
 */
import { describe, it, expect, vi } from 'vitest'
import { createTrailingRowPane } from './trailing-row-pane'
import type { TrailingRow } from '../views/trailing-row'

const ROW: TrailingRow = { label: 'Make a thing…', commandId: 'servers.connect', mcpName: '+ Make a thing…' }

function pane(opts: { row?: TrailingRow; cursor?: number; entryRows?: number } = {}) {
  const state = { cursor: opts.cursor ?? 0, entryRows: opts.entryRows ?? 4 }
  const runCommand = vi.fn()
  const trailing = createTrailingRowPane({
    getRow: () => ('row' in opts ? opts.row : ROW),
    getEntryRows: () => state.entryRows,
    getCursorIndex: () => state.cursor,
    runCommand,
  })
  return { trailing, runCommand, state }
}

describe('createTrailingRowPane', () => {
  it('spans one more cursor row than the entries, and selection stops at the row', () => {
    const { trailing } = pane()
    expect(trailing.cursorRows()).toBe(5)
    expect(trailing.selectableEnd()).toBe(4)
  })

  it('Enter on it runs its command; Enter elsewhere leaves the entry to open', () => {
    const { trailing, runCommand, state } = pane({ cursor: 4 })
    expect(trailing.isUnderCursor()).toBe(true)
    expect(trailing.activateUnderCursor()).toBe(true)
    expect(runCommand).toHaveBeenCalledWith('servers.connect')
    state.cursor = 3
    expect(trailing.activateUnderCursor()).toBe(false)
    expect(runCommand).toHaveBeenCalledTimes(1)
  })

  it('MCP `move_cursor` finds it by its MCP name or its words, at its screen index', () => {
    const { trailing } = pane()
    expect(trailing.indexNamed('+ Make a thing…')).toBe(4)
    expect(trailing.indexNamed('Make a thing…')).toBe(4)
    expect(trailing.indexNamed('alpha')).toBeNull()
  })

  it('mirrors it last as an action with no path, only when the mirrored window reaches it', () => {
    const { trailing } = pane()
    expect(trailing.mcpEntry(4)).toBeNull()
    expect(trailing.mcpEntry(5)).toMatchObject({ name: '+ Make a thing…', path: '', action: 'servers.connect' })
  })

  it('keeps a cursor on it across a re-list, and never moves another cursor onto it', () => {
    const { trailing, state } = pane({ cursor: 4 })
    state.entryRows = 6
    expect(trailing.cursorAcrossRebuild({ cursorIndex: 4, previousEntryRows: 4 })).toBe(6)
    expect(trailing.cursorAcrossRebuild({ cursorIndex: 5, previousEntryRows: 7 })).toBe(5)
    state.entryRows = 3
    expect(trailing.cursorAcrossRebuild({ cursorIndex: 5, previousEntryRows: 7 })).toBe(2)
  })

  it('answers as a plain list does when the kind asks for no trailing row', () => {
    const { trailing, runCommand } = pane({ row: undefined, cursor: 4 })
    expect(trailing.cursorRows()).toBe(4)
    expect(trailing.selectableEnd()).toBe(Number.POSITIVE_INFINITY)
    expect(trailing.isUnderCursor()).toBe(false)
    expect(trailing.activateUnderCursor()).toBe(false)
    expect(trailing.indexNamed('+ Make a thing…')).toBeNull()
    expect(trailing.mcpEntry(10)).toBeNull()
    trailing.run()
    expect(runCommand).not.toHaveBeenCalled()
  })
})
