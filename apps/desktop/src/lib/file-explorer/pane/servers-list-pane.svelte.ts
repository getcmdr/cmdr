/**
 * What a pane showing the Servers list (the `network` volume at `smb://`) does beyond any
 * listing, keyed on the pane's LOCATION, ❌ never a component mount:
 *
 * - **The discovery hold**: showing the Servers list is a networking intent
 *   (`triggerNetworkDiscovery`, the first one records it), and the backend browses mDNS while
 *   some pane shows it, so hosts arrive and leave live there (`holdDiscoveryForServersView`).
 *   Both panes share one lease count; leaving `smb://` releases this pane's.
 * - **A server just added lands under the cursor**: "Add" in the sheet saves it with no pane
 *   moved, and the `volumes-changed` it emits re-lists `smb://` a moment later, so the row
 *   is selected by its OWN id (`ServerMeta.rowId`) once a re-list brings it, ❌ never by a
 *   name or an address. A row that never comes (another pane moved on, the save broke) stops
 *   waiting after {@link ADDED_ROW_WAIT_MS}.
 */

import { untrack } from 'svelte'
import { holdDiscoveryForServersView } from '../network/network-store.svelte'
import { triggerNetworkDiscovery } from '../network/lazy-trigger'
import type { FileEntry } from '../types'

/** How long a just-added server's row is waited for before the pane stops looking. */
const ADDED_ROW_WAIT_MS = 10_000

export interface ServersListPaneDeps {
  /** Whether the pane shows the Servers list (`capabilitiesForPane(…).kind === 'network'`). Reactive. */
  getIsServersList: () => boolean
  /** The pane's entry-row count, which a re-list that adds a row changes. Reactive. */
  getTotalCount: () => number
  /** The listing the rows belong to. Reactive. */
  getListingId: () => string
  /** The pane's rows, at the pane's own indices. */
  getEntriesSnapshot: () => Promise<FileEntry[]>
  setCursorIndex: (index: number) => void
}

export interface ServersListPane {
  /** Lands the cursor on the row whose id is `rowId`, now or once a re-list brings it. */
  selectRowWhenListed: (rowId: string) => void
}

export function createServersListPane(deps: ServersListPaneDeps): ServersListPane {
  $effect(() => {
    if (!deps.getIsServersList()) return
    return untrack(() => {
      triggerNetworkDiscovery()
      return holdDiscoveryForServersView()
    })
  })

  let pending: { rowId: string; timer: ReturnType<typeof setTimeout> } | null = null

  function stopWaiting(): void {
    if (pending) clearTimeout(pending.timer)
    pending = null
  }

  async function tryLanding(rowId: string): Promise<void> {
    if (!deps.getIsServersList()) return
    const entries = await deps.getEntriesSnapshot()
    if (pending?.rowId !== rowId) return
    const index = entries.findIndex((entry) => entry.serverMeta?.rowId === rowId)
    if (index < 0) return
    stopWaiting()
    deps.setCursorIndex(index)
  }

  // Each re-list that changes the rows is another chance for the waited-for row to be there.
  $effect(() => {
    deps.getTotalCount()
    deps.getListingId()
    untrack(() => {
      if (pending) void tryLanding(pending.rowId)
    })
  })

  function selectRowWhenListed(rowId: string): void {
    stopWaiting()
    pending = { rowId, timer: setTimeout(stopWaiting, ADDED_ROW_WAIT_MS) }
    void tryLanding(rowId)
  }

  // The pane going away stops the wait.
  $effect(() => stopWaiting)

  return { selectRowWhenListed }
}
