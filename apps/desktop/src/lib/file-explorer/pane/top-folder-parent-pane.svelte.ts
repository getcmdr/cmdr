/**
 * One pane's `..` out of a server's top folder: the ONE seam for every server kind. A share
 * list goes up to the Servers list, an SMB share to its share list (#1238), an SFTP, WebDAV,
 * or NFS place and an S3 account's root to the Servers list, and a bucket to its account's
 * bucket list. Each lands on the row it came from.
 *
 * ❌ No `if s3` / `if sftp` here: Rust says where `..` leads on every listing (and on a
 * failed one, the error screen's way up), with which switch shows it
 * (`src-tauri/src/network/top_folder_parent.rs`). `listing-loader.ts` notes the answer
 * BEFORE it hands over the rows, so `topFolderLeadsOut` is right in the rows' first frame
 * and no late `..` shifts every index by one. With the switch off, the top folder is a
 * root, as any volume's is: no `..` row, and Go to parent does nothing.
 *
 * The one kind-shaped answer is Rust's `switch`: a share leaves through its share list's
 * door (`openServer`: the cursor lands on the share by name and the hint explains where
 * `..` went), every other `..` through `goTo`, by path, in place or onto another volume.
 */

import type { NetworkHost, VolumeInfo } from '../types'
import type { ParentRowSwitch, TopFolderParent } from '#lib/tauri-commands/index.js'
import { getShowServerParentRow } from '#lib/settings/reactive-settings.svelte.js'
import {
  isSmbShareVolume,
  loadShareParent,
  shareParentOf,
  showsParentRow,
  type ShareParentLanding,
} from '../network/share-parent-row.svelte'

/** What a listing said about its folder's way up. */
export interface ListingTop {
  /** The folder the listing was for, as the pane spells it. */
  path: string
  /** Rust's "this folder is its volume's root"; `undefined` from a failed listing, which doesn't say. */
  atVolumeTop?: boolean
  /** Where `..` leads when it isn't the folder above, or `null`. */
  parent: TopFolderParent | null
}

export interface TopFolderParentPaneDeps {
  /** The volume the pane's `volumeId` names, from the live list. */
  getVolume: () => VolumeInfo | null
  /** The pane's current folder. */
  getCurrentPath: () => string
  /**
   * Takes the pane to `host`'s share list, the cursor landing on the share and the hint
   * explaining where `..` went once it does (`share-list-pane.svelte.ts`).
   */
  openServer: (host: NetworkHost, arrival: { land: ShareParentLanding }) => void
  /** Takes the pane to `parent.path` on `parent.volumeId`, the cursor landing on `parent.selectPath`. */
  goTo: (parent: TopFolderParent) => void
}

export interface TopFolderParentPane {
  /** Records what a listing (or a failed one) said about its folder's way up. */
  note: (top: ListingTop) => void
  /** Rust's "the current folder is its volume's root", or `undefined` until a listing of it said. */
  readonly atVolumeTop: boolean | undefined
  /**
   * Whether the current folder's `..` leads out of its volume (or a share list's up to the
   * Servers list) and its switch shows it. That row is no folder: ❌ no drop target, no
   * "Add to favorites", no file menu.
   */
  readonly leadsOutHere: boolean
  /** Where the `..` row leads while `leadsOutHere`, for the MCP mirror; else `null`. */
  readonly leadsTo: string | null
  /** Leaves the current folder through its `..` when that leads out. Answers whether it did. */
  leave: () => boolean
}

/** Whether `switchKind` shows the `..` on the volume `volumeId`. Reactive (the settings and the share cache). */
function switchShows(switchKind: ParentRowSwitch, volumeId: string | null): boolean {
  switch (switchKind) {
    case 'always':
      return true
    case 'share':
      return volumeId !== null && showsParentRow(volumeId) === true
    case 'server':
      return getShowServerParentRow()
  }
}

export function createTopFolderParentPane(deps: TopFolderParentPaneDeps): TopFolderParentPane {
  let top = $state<ListingTop | null>(null)

  const shareVolumeId = $derived.by(() => {
    const volume = deps.getVolume()
    return isSmbShareVolume(volume) ? volume.id : null
  })

  // Asked again each time the pane lands on a share, so a switch flipped elsewhere (the
  // other pane's switcher, a restart) and a server discovery has since found both show,
  // and the share's server is on record for its share list's label (`share-list-label.ts`).
  $effect(() => {
    if (shareVolumeId !== null) void loadShareParent(shareVolumeId)
  })

  /** What the listing of the CURRENT folder said; a listing of another folder says nothing about it. */
  const here = $derived(top !== null && top.path === deps.getCurrentPath() ? top : null)

  const parent = $derived.by(() => {
    const candidate = here?.parent ?? null
    if (!candidate) return null
    return switchShows(candidate.switch, deps.getVolume()?.id ?? null) ? candidate : null
  })

  function leave(): boolean {
    if (!parent) return false
    if (parent.switch === 'share') {
      const volumeId = shareVolumeId
      const share = volumeId === null ? null : shareParentOf(volumeId)
      if (!volumeId || !share) return false
      deps.openServer(share.host, {
        land: { volumeId, hostId: share.host.id, shareName: share.shareName, serverLabel: share.serverLabel },
      })
      return true
    }
    deps.goTo(parent)
    return true
  }

  return {
    note: (next) => {
      top = next
    },
    get atVolumeTop() {
      return here?.atVolumeTop
    },
    get leadsOutHere() {
      return parent !== null
    },
    get leadsTo() {
      return parent?.path ?? null
    },
    leave,
  }
}
