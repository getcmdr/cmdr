import { afterEach, describe, expect, it, vi } from 'vitest'
import { createPaneRowState } from './pane-row-state'

function readyRowState(overrides: { loading?: () => boolean; operationActive?: () => boolean } = {}) {
  const rowState = createPaneRowState({
    getListingId: () => 'listing-1',
    getLoading: overrides.loading ?? (() => false),
    getOperationActive: overrides.operationActive ?? (() => false),
  })
  rowState.initialize()
  return rowState
}

afterEach(() => {
  vi.useRealTimers()
})

// `whenReady` is how a command that needs settled rows (F5 / F6 with a selection) waits for
// them instead of dropping the press. It's driven by the transitions that can make the rows
// ready, never by polling.
describe('whenReady', () => {
  it('answers true at once when the rows are already settled', async () => {
    await expect(readyRowState().whenReady(1000)).resolves.toBe(true)
  })

  it('answers true as soon as in-flight row work ends', async () => {
    const rowState = readyRowState()
    const endWork = rowState.beginAsyncWork()
    let answer: boolean | undefined
    void rowState.whenReady(1000).then((value) => (answer = value))
    await Promise.resolve()
    expect(answer).toBeUndefined()

    endWork()
    await Promise.resolve()

    expect(answer).toBe(true)
  })

  it('answers true when an outside condition clears and the pane says so', async () => {
    let operationActive = true
    const rowState = readyRowState({ operationActive: () => operationActive })
    let answer: boolean | undefined
    void rowState.whenReady(1000).then((value) => (answer = value))

    operationActive = false
    rowState.recheck()
    await Promise.resolve()

    expect(answer).toBe(true)
  })

  it('answers false when the rows stay unsettled past the budget', async () => {
    vi.useFakeTimers()
    const rowState = readyRowState({ operationActive: () => true })
    const waiting = rowState.whenReady(1000)

    await vi.advanceTimersByTimeAsync(1000)

    await expect(waiting).resolves.toBe(false)
  })

  it('answers false at once when the pane is gone', async () => {
    const rowState = readyRowState({ operationActive: () => true })
    const waiting = rowState.whenReady(1000)

    rowState.dispose()

    await expect(waiting).resolves.toBe(false)
  })
})

// ERR-VC6Q7: five F6 presses ran out the settle budget with nothing changing on the
// backend, and the log couldn't say what held the rows. `unsettledBy` names it.
describe('unsettledBy', () => {
  it('names nothing when the rows are settled', () => {
    expect(readyRowState().unsettledBy()).toEqual([])
  })

  it('names every condition holding the rows at once', () => {
    const rowState = readyRowState({ loading: () => true, operationActive: () => true })
    rowState.beginAsyncWork()

    expect(rowState.unsettledBy()).toEqual(['loading', 'asyncWork', 'operation'])
    expect(rowState.isReady()).toBe(false)
  })

  it('names a diff whose predecessor never arrived', () => {
    const rowState = readyRowState()
    rowState.receive([
      { fromSequence: 3, sequence: 4, totalCount: 1, hiddenCount: 0, changes: [], effectiveSortBy: null },
    ])

    expect(rowState.unsettledBy()).toEqual(['diffGap'])
  })

  it('names rows that never finished their first load', () => {
    const rowState = createPaneRowState({
      getListingId: () => 'listing-1',
      getLoading: () => false,
      getOperationActive: () => false,
    })

    expect(rowState.unsettledBy()).toEqual(['notInitialized'])
  })
})
