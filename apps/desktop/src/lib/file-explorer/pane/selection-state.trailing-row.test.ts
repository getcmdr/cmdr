/**
 * A kind's trailing action row sits at the first index past the entries
 * (`../views/trailing-row.ts`): the cursor reaches it, and no gesture ever selects it.
 * Here: `..` at 0, entries 1–3, the trailing row at 4.
 */
import { describe, it, expect } from 'vitest'
import { createSelectionState } from './selection-state.svelte'

const withTrailingRow = () => createSelectionState({ getSelectableEnd: () => 4 })

describe('createSelectionState with a trailing row', () => {
  it('Space on it selects nothing', () => {
    const state = withTrailingRow()
    expect(state.toggleAt(4, true)).toBe(false)
    expect(state.getSelectedIndices()).toEqual([])
  })

  it('⇧↓ from the last entry onto it marks the entry, never the row', () => {
    const state = withTrailingRow()
    state.handleShiftKeyboardNavigation(3, 4, false, true)
    expect(state.getSelectedIndices()).toEqual([3])
    // ⇧↓ again, clamped on the row itself (overflow): still nothing past the entries.
    state.handleShiftKeyboardNavigation(4, 4, true, true)
    expect(state.getSelectedIndices()).toEqual([3])
  })

  it('⇧End from `..` marks every entry and stops short of it', () => {
    const state = withTrailingRow()
    state.handleShiftKeyboardNavigation(0, 4, true, true)
    expect(state.getSelectedIndices()).toEqual([1, 2, 3])
  })

  it('a ⇧-click range reaching it, or a range command over it, covers only entries', () => {
    const state = withTrailingRow()
    state.handleShiftMouseNavigation(4, 2, true)
    expect(state.getSelectedIndices()).toEqual([2, 3])
    state.clearSelection()
    state.selectRange(1, 4, true)
    expect(state.getSelectedIndices()).toEqual([1, 2, 3])
  })

  it('⌘A and invert span the entries alone, and a matcher naming its index adds nothing there', () => {
    const state = withTrailingRow()
    state.selectAll(true, 4)
    expect(state.getSelectedIndices()).toEqual([1, 2, 3])
    expect(state.isAllSelected(true, 4)).toBe(true)
    state.invertSelection(true, 4)
    expect(state.getSelectedIndices()).toEqual([])
    state.applyIndices([2, 4], 'add', true)
    expect(state.getSelectedIndices()).toEqual([2])
  })

  it('selects past the entries when no trailing row bounds it, as before', () => {
    const state = createSelectionState()
    expect(state.toggleAt(4, true)).toBe(true)
  })
})
