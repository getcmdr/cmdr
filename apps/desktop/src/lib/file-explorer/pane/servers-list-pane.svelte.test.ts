/**
 * A pane showing the Servers list holds the mDNS browse by its LOCATION (entering `smb://`
 * holds it, leaving releases it, once each), and lands the cursor on a just-added server's
 * row by the row's own id once a re-list brings it.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { flushSync } from 'svelte'
import type { FileEntry } from '../types'

const { release, hold, trigger } = vi.hoisted(() => {
  const release = vi.fn()
  return { release, hold: vi.fn(() => release), trigger: vi.fn() }
})
vi.mock('../network/network-store.svelte', () => ({ holdDiscoveryForServersView: hold }))
vi.mock('../network/lazy-trigger', () => ({ triggerNetworkDiscovery: trigger }))

import { createServersListPane } from './servers-list-pane.svelte'

function row(rowId: string): FileEntry {
  return { name: rowId, path: `smb://${rowId}`, serverMeta: { rowId } } as unknown as FileEntry
}

function harness(rows: FileEntry[] = []) {
  const state = $state({ isServersList: false, totalCount: rows.length, listingId: 'listing-1', rows })
  const setCursorIndex = vi.fn()
  let pane: ReturnType<typeof createServersListPane> | undefined
  const cleanup = $effect.root(() => {
    pane = createServersListPane({
      getIsServersList: () => state.isServersList,
      getTotalCount: () => state.totalCount,
      getListingId: () => state.listingId,
      getEntriesSnapshot: () => Promise.resolve(state.rows),
      setCursorIndex,
    })
  })
  flushSync()
  if (!pane) throw new Error('the factory ran')
  return { state, pane, setCursorIndex, cleanup }
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('the discovery hold', () => {
  it('holds the browse while the pane shows the Servers list, and releases it on leaving', () => {
    const { state, cleanup } = harness()
    expect(hold).not.toHaveBeenCalled()

    state.isServersList = true
    flushSync()
    expect(trigger).toHaveBeenCalledOnce()
    expect(hold).toHaveBeenCalledOnce()

    // A re-list (the count changing) is no second hold.
    state.totalCount = 4
    flushSync()
    expect(hold).toHaveBeenCalledOnce()

    state.isServersList = false
    flushSync()
    expect(release).toHaveBeenCalledOnce()
    cleanup()
  })
})

describe('selectRowWhenListed', () => {
  it('lands on the added row by its own id once a re-list brings it', async () => {
    const { state, pane, setCursorIndex, cleanup } = harness([row('attic')])
    state.isServersList = true
    flushSync()

    pane.selectRowWhenListed('manual-testnas')
    await vi.waitFor(() => Promise.resolve())
    expect(setCursorIndex).not.toHaveBeenCalled()

    state.rows = [row('attic'), row('manual-testnas')]
    state.totalCount = 2
    flushSync()
    await vi.waitFor(() => {
      expect(setCursorIndex).toHaveBeenCalledWith(1)
    })
    cleanup()
  })
})
