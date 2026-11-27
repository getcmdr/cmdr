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

// The counters `isReady` reads belong to the listing they were taken for. Work still in
// flight for a listing the pane has left (a lookup, a re-sort waiting on its IPC answer)
// must not hold the NEXT listing's rows, which would refuse F5/F6 on a folder nothing is
// happening in (ERR-VC6Q7's suspect).
describe('work for a listing the pane has left', () => {
  let listingId = 'listing-1'
  function navigableRowState() {
    listingId = 'listing-1'
    const rowState = createPaneRowState({
      getListingId: () => listingId,
      getLoading: () => false,
      getOperationActive: () => false,
    })
    rowState.initialize()
    return rowState
  }
  function navigate(rowState: ReturnType<typeof createPaneRowState>) {
    listingId = 'listing-2'
    rowState.reset()
    rowState.initialize()
  }

  it("doesn't hold the next listing while an old lookup is still out", () => {
    const rowState = navigableRowState()
    const endOldWork = rowState.beginAsyncWork()

    navigate(rowState)
    expect(rowState.unsettledBy()).toEqual([])

    // The old lookup lands late: it ends its own lease, not some new one.
    const endNewWork = rowState.beginAsyncWork()
    endOldWork()
    expect(rowState.unsettledBy()).toEqual(['asyncWork'])
    endNewWork()
    expect(rowState.isReady()).toBe(true)
  })

  it("doesn't hold the next listing while an old view change waits on its answer", async () => {
    const rowState = navigableRowState()
    let answerOld: (result: { sequence: number }) => void = () => {}
    const oldChange = rowState.changeView({
      request: () => new Promise<{ sequence: number }>((resolve) => (answerOld = resolve)),
      install: () => undefined,
    })
    await Promise.resolve()

    navigate(rowState)
    expect(rowState.unsettledBy()).toEqual([])

    // A view change on the new listing runs now, never queued behind the old answer.
    const newChange = rowState.changeView({ request: () => Promise.resolve({ sequence: 0 }), install: () => undefined })
    await expect(newChange).resolves.toBe(true)

    answerOld({ sequence: 9 })
    await expect(oldChange).resolves.toBe(false)
    expect(rowState.getSequence()).toBe(0)
    expect(rowState.isReady()).toBe(true)
  })

  it('releases a view change whose request fails', async () => {
    const rowState = navigableRowState()
    const failing = rowState.changeView({ request: () => Promise.reject(new Error('gone')), install: () => undefined })

    await expect(failing).rejects.toThrow('gone')
    expect(rowState.isReady()).toBe(true)
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
