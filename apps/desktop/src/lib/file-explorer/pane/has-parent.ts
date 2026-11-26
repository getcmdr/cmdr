import { type CanonicalPath, parentOf } from '#lib/path/canonical.js'
import { NETWORK_VOLUME_PATH } from './navigate-refusals'
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
   * Whether the volume's top folder has a `..` leading out of the volume: an SMB share
   * whose "Show “..” in the top folder" is on, where `..` opens the server's shares
   * (#1238, `../network/share-parent-row.svelte.ts`). Omitted: a root has no `..`.
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
  if (input.currentPath === input.effectiveVolumeRoot) return input.topFolderLeadsOut === true
  return true
}

/**
 * The folder a `..` row leads to. A server's share list (`smb://<server>`) leads to the
 * Servers list at `smb://`, which the path grammar's slash arithmetic can't produce
 * (`parentOf('smb://testnas')` is `smb:/`, a path no volume answers).
 */
export function parentRowPath(kind: VolumeKind, canonicalPath: CanonicalPath): string {
  return kind === 'share-list' ? NETWORK_VOLUME_PATH : parentOf(canonicalPath)
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
  const routed = kind === 'archive' || kind === 'git-portal' || kind === 'restic'
  return routed ? volumePath : (volumeRootFromEvent ?? volumePath)
}
