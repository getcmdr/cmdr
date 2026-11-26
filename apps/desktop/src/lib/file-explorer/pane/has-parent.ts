import { type CanonicalPath, parentOf } from '#lib/path/canonical.js'
import type { VolumeKind } from './volume-capabilities'

/**
 * Pure helpers for `FilePane.svelte`'s `hasParent` derivation.
 *
 * The snapshot rule comes in as the `hasParentRow` capability
 * (`VolumeCapabilities.hasParentRow`, `false` for the search-results kind), NOT
 * a `volumeId === 'search-results'` string compare. A snapshot
 * pane has no `..` row, and the path comparison `currentPath !==
 * effectiveVolumeRoot` can't catch it — a `search-results://sr-N` URL never
 * matches a real volume root, so without `hasParentRow` the comparison returns
 * `true` and `selection.selectAll(hasParent, ...)` then skips index 0 (the
 * off-by-one pinned by the regression test).
 *
 * `hasParentRow` folds ONLY the snapshot rule; it is NOT a complete has-parent
 * answer. The two PATH comparisons stay here: a `local` pane at `/`, or any pane
 * sitting on its volume root, has no `..` despite `hasParentRow: true`. The real
 * answer is `hasParentRow && currentPath !== '/' && currentPath !== root`.
 */
export interface HasParentInput {
  /**
   * The pane kind's `hasParentRow` capability: `false` for the search-results
   * (snapshot) kind, `true` otherwise. Folds the snapshot rule only.
   */
  hasParentRow: boolean
  /** The pane's current path (filesystem path OR a virtual-volume URL). */
  currentPath: string
  /** The effective volume root (resolved from the listing event or the prop). */
  effectiveVolumeRoot: string
  /**
   * Rust's answer to "is the current folder its volume's root" (`listing-complete`'s
   * `atVolumeTop`), which wins over the path compare: a server root's spellings drift
   * (`s3://k@h:443` beside `s3://k@h:443/`). `undefined` until a listing of the folder said.
   */
  atVolumeTop?: boolean
  /**
   * Whether the current folder's `..` leads out of its volume and its switch shows it: a
   * server's top folder (an SMB share, an SFTP place, a bucket) or a share list
   * (`top-folder-parent-pane.svelte.ts`). Omitted: a root has no `..`.
   */
  topFolderLeadsOut?: boolean
}

/**
 * Returns whether the pane should render the `..` parent row. Pure: takes a
 * struct and returns a boolean. The contract:
 *   - Kinds without a `..` row (the snapshot kind, `hasParentRow: false`) NEVER
 *     have a parent row. Returning `false` here is what fixes the round-2 P6
 *     off-by-one in `selectAll`.
 *   - Filesystem root (`'/'`) has no parent.
 *   - When `currentPath === effectiveVolumeRoot` we're at the volume root,
 *     so there's no in-volume parent either, unless the top folder leads out
 *     (`topFolderLeadsOut`: an SMB share's `..` to its server's shares).
 *   - Everything else has a parent.
 */
export function computeHasParent(input: HasParentInput): boolean {
  if (!input.hasParentRow) return false
  if (input.currentPath === '/') return false
  if (input.topFolderLeadsOut === true) return true
  return !(input.atVolumeTop ?? input.currentPath === input.effectiveVolumeRoot)
}

/**
 * The folder a `..` row leads to: where it leads out of a server's top folder (`leadsTo`:
 * a share list's Servers list at `smb://`, a share's share list, a bucket's account), else
 * the folder above.
 */
export function parentRowPath(leadsTo: string | null, canonicalPath: CanonicalPath): string {
  return leadsTo ?? parentOf(canonicalPath)
}

/**
 * The root `..` stops at, for `computeHasParent`.
 *
 * Prefer the listing event's root (accurate for a phone's storage), falling back
 * to the drive's mount before one arrives. A ROUTED pane is the exception: the
 * backend reports the container as the listing's root (the `.zip`, a repo's
 * `Browse snapshots` folder, a `.git`), but the pane sits on the PARENT drive (the
 * tab keeps its id), and the container's top folder has a parent. So the drive's
 * mount is the root there, and `..` walks back out of the container.
 */
export function effectiveVolumeRootFor(
  kind: VolumeKind,
  volumePath: string,
  volumeRootFromEvent: string | undefined,
): string {
  return isRoutedKind(kind) ? volumePath : (volumeRootFromEvent ?? volumePath)
}

/**
 * Whether `kind` is a ROUTED pane: one sitting on its parent drive while a container lists
 * (a `.zip`, a repo's snapshots, a `.git`). Its listing's "top of the volume" is the
 * container's, whose top folder has a `..` back out, so `computeHasParent` ignores it there.
 */
export function isRoutedKind(kind: VolumeKind): boolean {
  return kind === 'archive' || kind === 'git-portal' || kind === 'restic'
}
