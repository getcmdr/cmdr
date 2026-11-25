/**
 * One pane's side of `..` in an SMB share's top folder (#1238): whether the top folder
 * shows `..`, and where `..` (and Go to parent, which follows the same switch) takes
 * the pane out of it: the server's share list, the cursor on the share it came from.
 *
 * The share's answer comes from `../network/share-parent-row.svelte.ts`, asked again
 * each time the pane lands on a share, and carried by every top-folder listing, which
 * `listing-loader.ts` notes BEFORE it hands over the rows: so `topFolderLeadsOut` is
 * already right in the rows' first frame, and no late `..` shifts every index by one.
 * With the switch off, the top folder is a root, as any volume's is: no `..` row, and
 * Go to parent does nothing.
 */

import type { NetworkHost, VolumeInfo } from '../types'
import {
  isSmbShareVolume,
  loadShareParent,
  shareParentOf,
  showsParentRow,
  type ShareParentLanding,
} from '../network/share-parent-row.svelte'
import { showShareParentHintOnce } from '../network/share-parent-hint'
import type { VolumeChangePayload } from './types'

export interface ShareParentPaneDeps {
  /** The volume the pane's `volumeId` names, from the live list. */
  getVolume: () => VolumeInfo | null
  /** Whether the pane shows its volume's top folder. */
  getIsAtTopFolder: () => boolean
  /** Takes the pane to the network volume on `networkHost`'s share list. */
  openServer: (change: VolumeChangePayload & { networkHost: NetworkHost }) => void
  getLanding: () => ShareParentLanding | undefined
  setLanding: (landing: ShareParentLanding | undefined) => void
}

export interface ShareParentPane {
  /** Whether the pane's volume is an SMB share whose top folder shows `..`. */
  readonly topFolderLeadsOut: boolean
  /**
   * Whether the pane's `..` row leads out of the share right now (top folder, switch on).
   * That row is no folder: ❌ no drop target, no "Add to favorites", no parent path.
   */
  readonly leadsOutHere: boolean
  /**
   * Leaves the share's top folder for the server's share list, when that's where `..`
   * leads from here. Answers whether it did; `false` leaves the caller's own parent
   * navigation to run.
   */
  leaveTopFolder: () => boolean
  /** The share list is on screen with the cursor on the share: spend the landing. */
  landed: () => void
}

export function createShareParentPane(deps: ShareParentPaneDeps): ShareParentPane {
  const shareVolumeId = $derived.by(() => {
    const volume = deps.getVolume()
    return isSmbShareVolume(volume) ? volume.id : null
  })

  // Asked again each time the pane lands on a share, so a switch flipped elsewhere (the
  // other pane's switcher, a restart) and a server discovery has since found both show.
  $effect(() => {
    if (shareVolumeId !== null) void loadShareParent(shareVolumeId)
  })

  const topFolderLeadsOut = $derived(shareVolumeId !== null && showsParentRow(shareVolumeId) === true)

  const leadsOutHere = $derived(topFolderLeadsOut && deps.getIsAtTopFolder())

  function leaveTopFolder(): boolean {
    if (shareVolumeId === null || !leadsOutHere) return false
    const parent = shareParentOf(shareVolumeId)
    if (!parent) return false
    // ❗ The switch first, then the landing: the pane drops its network state whenever it
    // isn't on the network volume, so a landing set before would be cleared at once.
    deps.openServer({ volumeId: 'network', volumePath: 'smb://', targetPath: 'smb://', networkHost: parent.host })
    deps.setLanding({
      volumeId: shareVolumeId,
      hostId: parent.host.id,
      shareName: parent.shareName,
      serverLabel: parent.serverLabel,
    })
    return true
  }

  function landed(): void {
    const landing = deps.getLanding()
    deps.setLanding(undefined)
    if (landing) showShareParentHintOnce(landing)
  }

  return {
    get topFolderLeadsOut() {
      return topFolderLeadsOut
    },
    get leadsOutHere() {
      return leadsOutHere
    },
    leaveTopFolder,
    landed,
  }
}
