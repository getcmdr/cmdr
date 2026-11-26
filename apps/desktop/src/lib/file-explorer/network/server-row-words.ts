/**
 * The words a Servers row and a saved share's row say, from the facts Rust ships on the
 * entry (`ServerMeta`, `ShareMeta.saved`): the Details cell, the status words, and the dot
 * the row's icon wears. Pure, and the ONE source of each: the cell renderer, the measurer
 * (through `extCellText`), the row's accessible name, Brief's tooltip, and the MCP tokens
 * all read it, ❌ never a word of their own.
 */

import type { MessageKey } from '#lib/intl/keys.gen.js'
import type { ServerMeta, ServerStatus } from '#lib/ipc/bindings.js'
import { tString } from '#lib/intl/messages.svelte.js'
import { protocolLabel } from '../navigation/filesystem-label'
import type { ConnectionState, FileEntry } from '../types'
import { signedInAsLabel } from './signed-in-as-label'

/** The word each status wears, the Servers hub's own. */
const STATUS_KEYS: Record<ServerStatus, MessageKey> = {
  connected: 'servers.hub.status.connected',
  saved: 'servers.hub.status.saved',
  not_connected_yet: 'servers.hub.status.notConnectedYet',
  found_nearby: 'servers.hub.status.foundNearby',
  signed_out: 'servers.hub.status.signedOut',
  waiting_for_key: 'servers.hub.status.waitingForKey',
}

/**
 * The dot each status draws, in the `ConnectionDot` colors: green live, amber asking, red a
 * changed host key, hollow saved. `null` for a host Cmdr only found: it's no place of the
 * person's, so there's nothing to be live or idle about.
 */
const STATUS_DOTS: Record<ServerStatus, ConnectionState | null> = {
  connected: 'direct',
  saved: 'saved',
  not_connected_yet: 'saved',
  found_nearby: null,
  signed_out: 'needs_sign_in',
  waiting_for_key: 'needs_host_key_approval',
}

/** What a status says, for the row's accessible name and tooltip. */
export function serverStatusWords(status: ServerStatus): string {
  return tString(STATUS_KEYS[status])
}

/** The Details cell of a Servers row: "SMB · 192.168.1.111 · as testuser". */
export function wordServerDetails(meta: ServerMeta): string {
  const protocol = protocolLabel(meta.protocol) ?? meta.protocol.toUpperCase()
  return meta.account
    ? tString('fileExplorer.network.servers.detailsWithAccount', {
        protocol,
        address: meta.address,
        account: signedInAsLabel(meta.account),
      })
    : tString('fileExplorer.network.servers.details', { protocol, address: meta.address })
}

/** A row's status dot: its color and the words it stands for. */
export interface RowStatusBadge {
  state: ConnectionState
  /** What the dot says, which the row's accessible name carries (❌ never a second live region). */
  label: string
}

/** The status a row shows: a Servers row's server, or a saved share's place. `null` for any other row. */
export function rowStatusOf(entry: FileEntry): ServerStatus | null {
  return entry.serverMeta?.status ?? entry.shareMeta?.saved?.status ?? null
}

/**
 * What a row's status says, for its accessible name, or `null` for a row with none. Said
 * even where no dot is drawn ("Found nearby"), since the row shows it nowhere else.
 */
export function rowStatusWords(entry: FileEntry): string | null {
  const status = rowStatusOf(entry)
  return status === null ? null : serverStatusWords(status)
}

/** The dot a row's icon wears, or `null` for a row with none. */
export function rowStatusBadge(entry: FileEntry): RowStatusBadge | null {
  const status = rowStatusOf(entry)
  const state = status === null ? null : STATUS_DOTS[status]
  return status === null || state === null ? null : { state, label: serverStatusWords(status) }
}
