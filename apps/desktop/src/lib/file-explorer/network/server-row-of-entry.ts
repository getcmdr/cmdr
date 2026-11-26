/**
 * The Servers row under a pane's cursor, as the row every server action takes (`HubRow`:
 * Edit, F8's Forget, the row menu, the palette's server commands): the hub's own cursor row
 * while the hub renders, else the listing's cursor entry, read from its `ServerMeta`.
 *
 * ❗ The saved entry is found by the row's OWN id (`ServerMeta.rowId`), ❌ never by a host,
 * a name, or an address (`DETAILS.md` § "Context menu and F8"): the stores are re-read at
 * action time, so an Edit opens the entry as it is now. The discovered host is the one
 * discovery holds under that id; a saved SMB server mDNS doesn't hold under its id stands
 * for the host its address makes (`smbHostOf` → `savedHostFor`), as its share list does.
 */

import { listSavedServers, type SavedServer } from '#lib/tauri-commands/index.js'
import { getAppLogger } from '#lib/logging/logger.js'
import type { FileEntry, NetworkHost } from '../types'
import type { NetworkCursorEntry } from '../pane/types'
import type { HubRow } from './servers-hub-rows'
import { getNetworkHosts } from './network-store.svelte'

const log = getAppLogger('servers')

/** What a Servers row's facts make of it, given the saved stores and discovery as they are now. */
export function serverRowOf(entry: FileEntry, saved: SavedServer[], hosts: NetworkHost[]): HubRow | null {
  const meta = entry.serverMeta
  if (!meta) return null
  return {
    id: meta.rowId,
    kind: 'server',
    parentId: null,
    account: meta.account,
    place: null,
    name: entry.name,
    protocol: meta.protocol,
    address: meta.address,
    status: meta.status,
    lastConnectedAt: entry.modifiedAt == null ? null : new Date(entry.modifiedAt * 1000).toISOString(),
    volumeId: meta.place,
    pinned: meta.pinned,
    saved: meta.saved ? (saved.find((server) => server.id === meta.rowId) ?? null) : null,
    host: hosts.find((host) => host.id === meta.rowId) ?? null,
  }
}

/** {@link serverRowOf} with the stores read now. A store that doesn't answer reads as nothing saved. */
export async function readServerRowOf(entry: FileEntry): Promise<HubRow | null> {
  if (!entry.serverMeta) return null
  const saved = await listSavedServers().catch((error: unknown) => {
    log.warn('Reading the saved servers for the row {name} broke down: {error}', { name: entry.name, error })
    return []
  })
  return serverRowOf(entry, Array.isArray(saved) ? saved : [], getNetworkHosts())
}

/** What the lookup reads off a pane. */
export interface ServerRowPane {
  /** The hub's cursor entry while the hub renders, else `null`. */
  getNetworkCursorEntry(): NetworkCursorEntry | null
  /** The listing's cursor entry, re-read so it's never the row the cursor just left. */
  refreshCursorEntry(): Promise<FileEntry | null>
}

/**
 * The Servers row under `pane`'s cursor, or `null` off the Servers list, on "Add server…",
 * or on a row with no server behind it.
 */
export async function serverRowUnderCursor(pane: ServerRowPane | undefined): Promise<HubRow | null> {
  if (!pane) return null
  const hub = pane.getNetworkCursorEntry()
  if (hub) return hub.row
  const entry = await pane.refreshCursorEntry()
  return entry ? readServerRowOf(entry) : null
}
