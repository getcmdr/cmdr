import { describe, it, expect } from 'vitest'
import {
  clampCursorAcrossRebuild,
  cursorRowCount,
  isTrailingRowIndex,
  trailingRowCount,
  trailingRowIndex,
  type TrailingRow,
} from './trailing-row'

const ROW: TrailingRow = { label: 'Make a thing…', commandId: 'servers.connect', mcpName: '+ Make a thing…' }

describe('trailing row counts', () => {
  it('adds one row after the entries (`..` included) only when the kind asks for it', () => {
    expect(trailingRowCount(ROW)).toBe(1)
    expect(trailingRowCount(undefined)).toBe(0)
    // `..` plus three entries: four entry rows, the trailing row at index 4.
    expect(cursorRowCount(4, ROW)).toBe(5)
    expect(cursorRowCount(4, undefined)).toBe(4)
    expect(trailingRowIndex(4, ROW)).toBe(4)
    expect(trailingRowIndex(4, undefined)).toBeNull()
    expect(isTrailingRowIndex(4, 4, ROW)).toBe(true)
    expect(isTrailingRowIndex(3, 4, ROW)).toBe(false)
    expect(isTrailingRowIndex(4, 4, undefined)).toBe(false)
  })

  it('puts it at index 0 on a list with no entries', () => {
    expect(trailingRowIndex(0, ROW)).toBe(0)
    expect(cursorRowCount(0, ROW)).toBe(1)
  })
})

describe('clampCursorAcrossRebuild', () => {
  it('keeps a cursor ON the trailing row on it as the entries grow or shrink', () => {
    expect(clampCursorAcrossRebuild({ cursorIndex: 3, previousEntryRows: 3, entryRows: 5, row: ROW })).toBe(5)
    expect(clampCursorAcrossRebuild({ cursorIndex: 3, previousEntryRows: 3, entryRows: 1, row: ROW })).toBe(1)
    expect(clampCursorAcrossRebuild({ cursorIndex: 3, previousEntryRows: 3, entryRows: 0, row: ROW })).toBe(0)
  })

  it('clamps a cursor whose row vanished to the last entry, never onto the trailing row', () => {
    expect(clampCursorAcrossRebuild({ cursorIndex: 8, previousEntryRows: 10, entryRows: 5, row: ROW })).toBe(4)
    expect(clampCursorAcrossRebuild({ cursorIndex: 2, previousEntryRows: 10, entryRows: 5, row: ROW })).toBe(2)
  })

  it('clamps as before when the kind has no trailing row', () => {
    expect(clampCursorAcrossRebuild({ cursorIndex: 8, previousEntryRows: 10, entryRows: 5, row: undefined })).toBe(4)
    expect(clampCursorAcrossRebuild({ cursorIndex: 3, previousEntryRows: 3, entryRows: 0, row: undefined })).toBe(0)
  })
})
