/** Which listed volume holds an OS path, by its row's root: the one rule navigation and the transfer target share. */
import type { VolumeInfo } from '../types'
import { isPathOnVolume } from '#lib/path/canonical.js'
import { paneRowsAreOsVisible, volumeKindOf } from './volume-capabilities'

/**
 * The listed volume whose root holds `path`, the longest root winning, among the rows whose
 * paths are OS paths: local drives AND shares (a mapped drive's row is a `network` one, read
 * off the row by `volumeKindOf`, so a share never passes for "not this machine's", #1384).
 * `undefined` when no listed root holds it: a share nobody mapped, or a drive the list
 * hasn't caught up with.
 *
 * By whole components and case-sensitively, the reading `commitPathFromListing`'s drop
 * policy takes, so a path this homes on a volume is never dropped as its foreign listing.
 */
export function listedOsVolumeHolding(path: string, volumes: readonly VolumeInfo[]): VolumeInfo | undefined {
  return volumes
    .filter(
      (v) =>
        v.category !== 'favorite' &&
        paneRowsAreOsVisible(volumeKindOf(v.id, v.fsType, v.category)) &&
        isPathOnVolume(path, v.path),
    )
    .sort((a, b) => b.path.length - a.path.length)[0]
}
