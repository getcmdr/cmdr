/**
 * Which server the palette's server commands act on.
 *
 * ❗ **The Servers list IS a pane**, so a command reading "the focused pane's volume"
 * would answer the synthetic Servers row rather than the server the cursor is on. The
 * rule is: the Servers row under the cursor wherever there is one, else the server whose share list
 * the pane shows, else the pane's own volume, and nothing at all when none of them
 * names a server the servers family speaks for.
 *
 * Pure, so the rule is one table of cells rather than a thing to reason about at
 * every call site.
 */

import { isServerPlaceRow } from '#lib/file-explorer/navigation/server-row-actions.js'
import type { ServerRow } from '#lib/file-explorer/network/server-row-of-entry.js'
import type { NetworkHost, VolumeInfo } from '#lib/file-explorer/types.js'

/**
 * The server a command will act on: a PLACE (a one-place server, an S3 place, a saved
 * share), which the servers family reaches by its `volumeId`, or an SMB SERVER, which has
 * no volume of its own (its places are mounted shares with ids `statfs` mints) and is
 * reached by its host. ❌ Never a fake `volumeId` for a host.
 */
export type ServerCommandTarget =
  | {
      kind: 'place'
      volumeId: string
      /** What to call it in a confirmation or a toast. */
      name: string
      /**
       * Whether it is pinned to the switcher, or `null` when this reading can't say.
       *
       * A `VolumeInfo` carries no pin, so a target resolved from the pane's volume
       * answers `null` and the caller reads the saved list before flipping it.
       */
      pinned: boolean | null
    }
  | {
      kind: 'smb-server'
      host: NetworkHost
      /** What to call it in the sheet and the toasts. */
      name: string
      /** The Servers row it was read off, or `null` when it's the server whose share list the pane shows. */
      row: ServerRow | null
    }

/** What the resolver reads. */
export interface ServerCommandSources {
  /** The Servers row under the focused pane's cursor, an SMB server's included, when that pane is the Servers list. */
  cursorRow: ServerRow | null
  /** The server whose share list the focused pane shows, or `null`. */
  shareListHost: NetworkHost | null
  /** The focused pane's own volume, from the volume list. */
  paneVolume: VolumeInfo | null
}

/** The server to act on, or `null` when nothing in view is one. */
export function serverCommandTarget(sources: ServerCommandSources): ServerCommandTarget | null {
  const row = sources.cursorRow
  if (row?.volumeId) {
    return { kind: 'place', volumeId: row.volumeId, name: row.name, pinned: row.pinned }
  }
  if (row?.host) {
    return { kind: 'smb-server', host: row.host, name: row.name, row }
  }
  // ❗ Any other row with no `volumeId` (an S3 account) stops the search rather than
  // falling through to the pane's volume: the user is pointing at that server, and
  // acting on something else instead is worse than doing nothing.
  if (row) return null

  const host = sources.shareListHost
  if (host) return { kind: 'smb-server', host, name: host.name, row: null }

  const volume = sources.paneVolume
  if (!volume || !isServerPlaceRow(volume)) return null
  return { kind: 'place', volumeId: volume.id, name: volume.name, pinned: null }
}
