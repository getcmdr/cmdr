/**
 * Enter (and a double-click, `openCursorItem`, MCP `open_under_cursor`) on a row the
 * `network` volume lists that leads OFF that volume, before the generic directory open sees
 * it. ❗ It must run first: a one-place server's row path is its place on another scheme
 * (`sftp://…`), and the generic open would navigate the `network` volume there.
 *
 * - A one-place server (SFTP, WebDAV, NFS): the pane lands on its place, a saved one at its
 *   start folder (`pathForPickedVolume`). The pane dials there (`place-connect`), so the
 *   wait and its Cancel render where every other wait does.
 * - An S3 account: its root (the place listing its buckets) is saved if it isn't yet, since
 *   Enter is the person asking for it, then the pane lands there.
 * - A saved share on a share list: its place, when the volume list holds one (mounted or
 *   pinned), so it opens as its saved account. Else the share list's own mount takes it.
 * - An SMB server: nothing here. Its row's path IS its share list on this volume.
 *
 * Pure apart from its deps, so every arm is a unit test.
 */

import { pathForPickedVolume } from '../navigation/picked-volume-path'
import type { FileEntry, VolumeInfo } from '../types'
import type { VolumeChangePayload } from './types'

export interface NetworkRowOpenDeps {
  onVolumeChange: (change: VolumeChangePayload) => void
  getVolumes: () => VolumeInfo[]
  /** `saveS3AccountRoot`: whether the account's root is saved now. */
  saveS3AccountRoot: (accountId: string) => Promise<boolean>
  /** Nothing under the account is saved to open (a race with a Forget): say so, by the row's name. */
  explainNoAccountRoot: (name: string) => void
}

/** Opens `entry` when it leads off the `network` volume; answers whether it took it. */
export function openNetworkRow(entry: FileEntry, deps: NetworkRowOpenDeps): boolean {
  const server = entry.serverMeta
  if (server) {
    if (server.place) {
      landOnPlace(server.place, entry.path, deps)
      return true
    }
    if (server.protocol === 's3') {
      void openS3Account(entry, server.rowId, deps)
      return true
    }
    return false
  }
  const saved = entry.shareMeta?.saved
  const volume = saved ? deps.getVolumes().find((candidate) => candidate.id === saved.volumeId) : undefined
  if (!volume) return false
  deps.onVolumeChange({ volumeId: volume.id, volumePath: volume.path, targetPath: pathForPickedVolume(volume) })
  return true
}

/** The pane on the place `volumeId`, at its start folder when the volume list knows it, else at `root`. */
function landOnPlace(volumeId: string, root: string, deps: NetworkRowOpenDeps): void {
  const volume = deps.getVolumes().find((candidate) => candidate.id === volumeId)
  deps.onVolumeChange({
    volumeId,
    volumePath: volume?.path ?? root,
    targetPath: volume ? pathForPickedVolume(volume) : root,
  })
}

async function openS3Account(entry: FileEntry, accountId: string, deps: NetworkRowOpenDeps): Promise<void> {
  const saved = await deps.saveS3AccountRoot(accountId).catch(() => false)
  if (!saved) {
    deps.explainNoAccountRoot(entry.name)
    return
  }
  // The row sits at the account's root, and the root's volume id IS the account's id.
  landOnPlace(accountId, entry.path, deps)
}
