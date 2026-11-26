/**
 * The Servers row under a pane's cursor, as the row every server action takes (`ServerRow`:
 * Edit, F8's Forget, the row menu, the palette's server commands), read off the listing's
 * cursor entry and its `ServerMeta`.
 *
 * ❗ The saved entry is found by the row's OWN id (`ServerMeta.rowId`), ❌ never by a host,
 * a name, or an address (`DETAILS.md` § "Acting on a Servers row"): the stores are re-read at
 * action time, so an Edit opens the entry as it is now.
 *
 * ❗ An SMB server's HOST is the one its share list resolves to, by the row's path (the
 * share-list path Rust wrote for the host the row opens): discovery's host under that path,
 * else the host the path itself names. So the row's credential status, listed account, and
 * sign-in are the share list's, a server saved by IP and shown under its Bonjour twin's name
 * included, ❌ never a host found by the row id or the Details text.
 */

import { listSavedServers, type SavedServer } from '#lib/tauri-commands/index.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { shareListHostKey } from '#lib/servers/share-list-path.js'
import type { ServerProtocol, ServerStatus } from '#lib/ipc/bindings.js'
import type { FileEntry, NetworkHost } from '../types'
import type { SignedInAs } from './signed-in-as'
import { shareListServerOf } from './share-list-label'

const log = getAppLogger('servers')

/** One server on the Servers list, as the actions on it read it. */
export interface ServerRow {
  /** The row's own id: the saved server's, else the discovered host's. */
  id: string
  /** The row's path: an SMB server's share-list path, a one-place server's app root, an S3 account's root. */
  path: string
  /** What the row's name reads (the friendly name). */
  name: string
  protocol: ServerProtocol
  /** Where the server is, as the Details cell shows it. */
  address: string
  status: ServerStatus
  /** The account the row is signed in as, `null` when nothing known says (`ServerMeta.account`). */
  account: SignedInAs | null
  /**
   * The place's volume id: a one-place server's. ❗ `null` for an SMB server (it opens its
   * share list) and an S3 account (it opens its root).
   */
  volumeId: string | null
  /** Whether the place is pinned to the switcher. Always `false` without a place. */
  pinned: boolean
  /** The saved entry behind the row, when the person saved one, by the row's own id. */
  saved: SavedServer | null
  /** An SMB server's host, the one its share list resolves to (module comment); `null` for any other server. */
  host: NetworkHost | null
}

/** What a Servers row's facts make of it, given the saved stores as they are now. */
export function serverRowOf(entry: FileEntry, saved: SavedServer[]): ServerRow | null {
  const meta = entry.serverMeta
  if (!meta) return null
  const row: ServerRow = {
    id: meta.rowId,
    path: entry.path,
    name: entry.name,
    protocol: meta.protocol,
    address: meta.address,
    status: meta.status,
    account: meta.account,
    volumeId: meta.place,
    pinned: meta.pinned,
    saved: meta.saved ? (saved.find((server) => server.id === meta.rowId) ?? null) : null,
    host: null,
  }
  return { ...row, host: smbHostOf(row) }
}

/**
 * The SMB server a Servers row stands for, or `null` for any other row: the host its share
 * list resolves to by the row's path (discovery's, else the one the path names, the way a
 * share list opened for a server nobody sees right now signs in).
 */
export function smbHostOf(row: Pick<ServerRow, 'id' | 'path' | 'name' | 'protocol'>): NetworkHost | null {
  if (row.protocol !== 'smb') return null
  const known = shareListServerOf(row.path)?.host
  if (known) return known
  const address = shareListHostKey(row.path)
  if (!address) return null
  return { id: row.id, name: row.name, hostname: address.key, port: address.port, source: 'manual' }
}

/** {@link serverRowOf} with the stores read now. A store that doesn't answer reads as nothing saved. */
export async function readServerRowOf(entry: FileEntry): Promise<ServerRow | null> {
  if (!entry.serverMeta) return null
  const saved = await listSavedServers().catch((error: unknown) => {
    log.warn('Reading the saved servers for the row {name} broke down: {error}', { name: entry.name, error })
    return []
  })
  return serverRowOf(entry, Array.isArray(saved) ? saved : [])
}

/** What the lookup reads off a pane. */
export interface ServerRowPane {
  /** The listing's cursor entry, re-read so it's never the row the cursor just left. */
  refreshCursorEntry(): Promise<FileEntry | null>
}

/**
 * The Servers row under `pane`'s cursor, or `null` off the Servers list, on "Add server…",
 * or on a row with no server behind it.
 */
export async function serverRowUnderCursor(pane: ServerRowPane | undefined): Promise<ServerRow | null> {
  const entry = await pane?.refreshCursorEntry()
  return entry ? readServerRowOf(entry) : null
}
