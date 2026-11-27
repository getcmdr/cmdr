/**
 * What the AI suggestions review says the disk holds now: drawn rows only, once per round, a new
 * round whenever something may have moved the files (#1524), and a gone file never ticked.
 * Fakes and fixtures: `test-suggested-ops-harness.ts`.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { SuggestedOpPage } from '#lib/tauri-commands/index.js'
import {
  BIG,
  approveMock,
  factsMock,
  group,
  listMock,
  listeners,
  ops,
  pageMock,
  resetHarness,
  sweep,
} from './test-suggested-ops-harness'

vi.mock('#lib/tauri-commands/index.js', async () => (await import('./test-suggested-ops-harness')).tauriCommandsMock())
vi.mock('#lib/logging/logger.js', async () => (await import('./test-suggested-ops-harness')).loggerMock())

import {
  INITIAL_ROWS,
  approvableCount,
  approveGroup,
  closeSuggestedOps,
  isNewNameTaken,
  isOpMissing,
  isOpSelected,
  opAt,
  openSuggestedOps,
  refreshSuggestions,
  reloadGroup,
  reviewOf,
  selectionState,
  showRange,
  suggestedOpsState,
  toggleAllOps,
  toggleOp,
} from './suggested-ops-trigger.svelte'

beforeEach(() => {
  resetHarness()
  closeSuggestedOps()
  suggestedOpsState.sweeps = []
  suggestedOpsState.loadError = false
})

describe('live facts', () => {
  it('checks only the rows the list drew, once each', async () => {
    await openSuggestedOps()

    expect(factsMock).toHaveBeenCalledTimes(1)
    expect(factsMock).toHaveBeenCalledWith({ groupId: BIG, offset: 0, limit: INITIAL_ROWS })

    factsMock.mockClear()
    await showRange(BIG, 0, INITIAL_ROWS)

    expect(factsMock).not.toHaveBeenCalled()
  })

  it('checks newly drawn rows as the list scrolls', async () => {
    await openSuggestedOps()
    factsMock.mockClear()

    await showRange(BIG, 5, 15)

    expect(factsMock).toHaveBeenCalledWith({ groupId: BIG, offset: 10, limit: 5 })
    expect(reviewOf(BIG)?.liveFacts.get(12)).toEqual({ kind: 'present', size: 2048, modified: 1_790_000_000 })
  })

  it('stops asking a source that isn’t connected, and says so once for the group', async () => {
    factsMock.mockResolvedValueOnce({ kind: 'refused', reason: { type: 'sourceNotConnected' } })
    await openSuggestedOps()
    factsMock.mockClear()

    await showRange(BIG, 5, 25)

    expect(reviewOf(BIG)?.sourceUnreachable).toBe('notConnected')
    expect(factsMock).not.toHaveBeenCalled()
  })

  it('asks again after a reload, in case the source came back', async () => {
    factsMock.mockResolvedValueOnce({ kind: 'refused', reason: { type: 'sourceGone' } })
    await openSuggestedOps()

    await reloadGroup(BIG)

    expect(reviewOf(BIG)?.sourceUnreachable).toBeNull()
    expect(reviewOf(BIG)?.liveFacts.size).toBe(INITIAL_ROWS)
  })

  it('asks again on the next scroll when the store refused', async () => {
    factsMock.mockResolvedValueOnce({ kind: 'refused', reason: { type: 'store', detail: 'database is locked' } })
    await openSuggestedOps()
    expect(reviewOf(BIG)?.liveFacts.size).toBe(0)

    await showRange(BIG, 0, INITIAL_ROWS)

    expect(reviewOf(BIG)?.liveFacts.size).toBe(INITIAL_ROWS)
  })

  it('asks again on the next scroll when a check threw', async () => {
    factsMock.mockRejectedValueOnce(new Error('share went away'))
    await openSuggestedOps()
    expect(reviewOf(BIG)?.liveFacts.size).toBe(0)

    await showRange(BIG, 0, INITIAL_ROWS)

    expect(reviewOf(BIG)?.liveFacts.size).toBe(INITIAL_ROWS)
  })
})

describe('a file that’s gone', () => {
  /** Live facts that say op 3 is gone and op 5's new name is taken, the rest present and free. */
  function factsSaying() {
    factsMock.mockImplementation(({ offset, limit }) =>
      Promise.resolve({
        kind: 'facts',
        facts: Array.from({ length: limit }, (_, i) => {
          const opId = offset + i
          return {
            opId,
            state: opId === 3 ? { kind: 'missing' } : { kind: 'present', size: 2048, modified: 1_790_000_000 },
            newName: opId === 5 ? 'taken' : null,
          }
        }),
      }),
    )
  }

  it('reads unticked, can’t be ticked back on, and counts for nothing', async () => {
    factsSaying()
    await openSuggestedOps()
    const gone = opAt(BIG, 3)
    if (!gone) throw new Error('row 3 is held')

    expect(isOpMissing(BIG, gone)).toBe(true)
    expect(isOpSelected(BIG, gone)).toBe(false)
    expect(approvableCount(BIG)).toBe(60_000 - 1)
    toggleOp(BIG, gone)
    expect(isOpSelected(BIG, gone)).toBe(false)
    expect(approvableCount(BIG)).toBe(60_000 - 1)
  })

  it('goes out turned off, as a flip over the stored ticks', async () => {
    factsSaying()
    await openSuggestedOps()

    await approveGroup(BIG)

    expect(approveMock).toHaveBeenCalledWith({
      groupId: BIG,
      revision: 1,
      selection: { mode: 'fromStored', excludeOpIds: [3], includeOpIds: [] },
    })
  })

  it('stays out after select all, and isn’t double-counted after select none', async () => {
    factsSaying()
    await openSuggestedOps()
    toggleAllOps(BIG) // all → none
    expect(approvableCount(BIG)).toBe(0)
    toggleAllOps(BIG) // none → all
    expect(approvableCount(BIG)).toBe(60_000 - 1)
    expect(selectionState(BIG)).toBe('all')

    await approveGroup(BIG)

    expect(approveMock).toHaveBeenCalledWith({
      groupId: BIG,
      revision: 1,
      selection: { mode: 'allExcept', opIds: [3] },
    })
  })

  /** From now on, the disk says every file is gone: another approval renamed them meanwhile. */
  function allGoneFromNow() {
    factsMock.mockImplementation(({ offset, limit }) =>
      Promise.resolve({
        kind: 'facts',
        facts: Array.from({ length: limit }, (_, i) => ({ opId: offset + i, state: { kind: 'missing' } })),
      }),
    )
  }

  /** Regression anchor for #1524 (ERR-BD64C): a group whose files an earlier approval in the
   *  same open dialog renamed kept its open-time "present" facts, so after its own approval was
   *  given back ("couldn't read any of these files") every row still read ticked, sized, and dated. */
  it('reads gone once a refused approval gives the group back, not as it was when the dialog opened', async () => {
    await openSuggestedOps()
    const first = opAt(BIG, 0)
    if (!first) throw new Error('row 0 is held')
    expect(isOpMissing(BIG, first)).toBe(false)
    allGoneFromNow()
    approveMock.mockResolvedValueOnce({ kind: 'nothingToRun' })

    await approveGroup(BIG)

    expect(isOpMissing(BIG, first)).toBe(true)
    expect(isOpSelected(BIG, first)).toBe(false)
    expect(reviewOf(BIG)?.liveFacts.get(0)).toEqual({ kind: 'missing' })
  })

  it('reads gone once another write settles while the dialog is open', async () => {
    await openSuggestedOps()
    const first = opAt(BIG, 0)
    if (!first) throw new Error('row 0 is held')
    allGoneFromNow()

    listeners.settle?.()
    await vi.waitFor(() => {
      expect(isOpMissing(BIG, first)).toBe(true)
    })
  })

  it('keeps the last answer on screen while it asks again, so a row never blinks to its snapshot', async () => {
    await openSuggestedOps()
    const hold: { answer: ((value: unknown) => void) | null } = { answer: null }
    factsMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          hold.answer = resolve
        }),
    )

    listeners.settle?.()

    expect(reviewOf(BIG)?.liveFacts.get(0)).toEqual({ kind: 'present', size: 2048, modified: 1_790_000_000 })
    hold.answer?.({ kind: 'facts', facts: [] })
  })

  it('drops an answer that a newer check overtook', async () => {
    await openSuggestedOps()
    const hold: { answer: ((value: unknown) => void) | null } = { answer: null }
    factsMock.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          hold.answer = resolve
        }),
    )
    listeners.settle?.() // asks, and the answer hangs
    allGoneFromNow()
    await refreshSuggestions() // asks again, and hears "gone"
    const first = opAt(BIG, 0)
    if (!first) throw new Error('row 0 is held')
    expect(isOpMissing(BIG, first)).toBe(true)

    hold.answer?.({ kind: 'facts', facts: [{ opId: 0, state: { kind: 'present', size: 1, modified: 1 } }] })
    await Promise.resolve()
    await Promise.resolve()

    expect(isOpMissing(BIG, first)).toBe(true)
  })

  it('marks a row whose new name is already taken where it lands (#774)', async () => {
    factsSaying()
    await openSuggestedOps()
    const taken = opAt(BIG, 5)
    const free = opAt(BIG, 4)
    if (!taken || !free) throw new Error('rows are held')

    expect(isNewNameTaken(BIG, taken)).toBe(true)
    expect(isNewNameTaken(BIG, free)).toBe(false)
  })

  // #1524: the re-check and the taken-over statuses (`superseded`, `handledElsewhere`) must not
  // fight. A gone taken-over row isn't counted out twice, and its file coming back never ticks it.
  it('leaves a taken-over row unticked and counted once through every re-check, gone or back', async () => {
    listMock.mockResolvedValue([sweep([group(BIG, 8, { totalOpCount: 10, retiredOpCount: 2 })])])
    pageMock.mockResolvedValue({
      ops: ops(0, 10).map((op) =>
        op.opId === 2 ? { ...op, status: 'superseded' } : op.opId === 3 ? { ...op, status: 'handledElsewhere' } : op,
      ),
      offset: 0,
      total: 10,
    } as unknown as SuggestedOpPage)
    const answer =
      (gone: number[]) =>
      ({ offset, limit }: { offset: number; limit: number }) =>
        Promise.resolve({
          kind: 'facts',
          facts: Array.from({ length: limit }, (_, i) => ({
            opId: offset + i,
            state: gone.includes(offset + i) ? { kind: 'missing' } : { kind: 'present', size: 1, modified: 1 },
          })),
        })
    factsMock.mockImplementation(answer([2, 4]))
    await openSuggestedOps()
    const retired = opAt(BIG, 2)
    if (!retired) throw new Error('row 2 is held')
    expect(retired.status).toBe('superseded')

    // Rows 2 (taken over) and 4 (ordinary) are gone: only row 4 comes off the eight.
    expect(isOpMissing(BIG, retired)).toBe(true)
    expect(approvableCount(BIG)).toBe(7)
    expect(selectionState(BIG)).toBe('all')
    toggleAllOps(BIG)
    toggleAllOps(BIG)
    expect(approvableCount(BIG)).toBe(7)

    // A write settles and row 2's file is back: it's still taken over, so still unticked.
    factsMock.mockImplementation(answer([4]))
    listeners.settle?.()
    await vi.waitFor(() => {
      expect(isOpMissing(BIG, retired)).toBe(false)
    })
    expect(isOpSelected(BIG, retired)).toBe(false)
    toggleOp(BIG, retired)
    expect(isOpSelected(BIG, retired)).toBe(false)
    expect(approvableCount(BIG)).toBe(7)

    // The wire turns off the gone ordinary row only; the taken-over one never runs anyway.
    await approveGroup(BIG)
    expect(approveMock).toHaveBeenCalledWith({
      groupId: BIG,
      revision: 1,
      selection: { mode: 'allExcept', opIds: [4] },
    })
  })
})
