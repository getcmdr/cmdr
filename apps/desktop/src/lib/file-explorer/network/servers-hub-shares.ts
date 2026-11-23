/**
 * The shares a saved SMB server has besides the ones the person saved, as rows
 * under it in the hub, plus the "More shares on …" row that stands last (#1404).
 *
 * A person who saved one share of a Mac read the server as having only that one:
 * the rows under a server were the saved shares alone, and nothing said there
 * were more. So a SAVED server lists every share its prefetched list holds.
 *
 * ❗ **Off the list Cmdr already prefetched, ❌ never a new listing.** Listing a
 * server signs in to it (#324), so these rows come only from `startPrefetchShares`'
 * result for a saved server, and a server Cmdr only found gets none.
 *
 * Pure, like `servers-hub-rows.ts`: `servers-hub-items.ts` splices these in.
 */

import { openMoveFor, savedHostFor, type HubOpenMove, type HubRow } from './servers-hub-rows'

/** How many share rows a server shows, saved ones included, before the rest go behind the More row. */
export const MAX_SHARE_ROWS_PER_SERVER = 8

/** A share of a saved server that isn't saved yet, or the "More shares on …" row under one. */
export type ShareItem =
  | { kind: 'unsaved_share'; id: string; server: HubRow; name: string }
  | { kind: 'more_shares'; id: string; server: HubRow }

/** What goes under one saved server: its unsaved shares, and whether the More row follows them. */
export interface ServerShareRows {
  /** The names, case-insensitively by name. */
  unsaved: string[]
  /** Whether the "More shares on …" row stands last. */
  more: boolean
}

/**
 * What goes under `server`, from the share names its prefetched list holds
 * (`null` while it's loading, needs a sign-in, couldn't be read, or was never
 * read), or `null` for a row that gets no share rows.
 *
 * ❗ The cap counts the saved shares too, but never hides one: a saved share is
 * a row the person had before this list existed. The More row stands whenever
 * a share isn't on screen, or nobody knows yet whether one is.
 */
export function shareRowsFor(server: HubRow, shares: readonly string[] | null): ServerShareRows | null {
  if (!listsOtherShares(server) || server.saved === null) return null
  if (shares === null) return { unsaved: [], more: true }
  const seen = new Set(server.saved.places.map((place) => fold(place.name)))
  const unsaved: string[] = []
  for (const name of shares) {
    const key = fold(name)
    if (seen.has(key)) continue
    seen.add(key)
    unsaved.push(name)
  }
  unsaved.sort((a, b) => a.localeCompare(b, undefined, { sensitivity: 'base' }))
  const room = Math.max(0, MAX_SHARE_ROWS_PER_SERVER - server.saved.places.length)
  return { unsaved: unsaved.slice(0, room), more: unsaved.length > room }
}

/**
 * Whether `row` gets share rows under it: a SAVED SMB server. A host Cmdr only
 * found keeps today's behavior, nothing listed until the person opens it (#324).
 */
export function listsOtherShares(row: HubRow): boolean {
  return row.kind === 'server' && row.protocol === 'smb' && row.saved !== null
}

/**
 * Where Enter on a share item leads. An unsaved share mounts through its host's
 * share list, as a saved share no mount went through does (`openMoveFor`'s
 * `share_via_host`), and that first mount is what saves it. The More row opens
 * the share list, exactly what Enter on the server row does.
 */
export function shareItemMove(item: ShareItem): HubOpenMove {
  const host = item.server.host ?? savedHostFor(item.server)
  if (item.kind === 'unsaved_share') {
    return { kind: 'share_via_host', host, share: item.name, label: item.server.name }
  }
  return openMoveFor(item.server, [], []) ?? { kind: 'host', host, label: item.server.name }
}

/** Case-insensitive and blind to Unicode form, as `list-cursor.ts::indexOfName` matches a share. */
function fold(name: string): string {
  return name.normalize('NFC').toLowerCase()
}
