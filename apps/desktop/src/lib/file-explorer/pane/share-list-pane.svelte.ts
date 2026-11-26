/**
 * A pane's side of a server's share list (`network` at `smb://<server>`): what it does that
 * a folder doesn't, at the seams a folder already has. Enter on a share MOUNTS it (the
 * share-mount controller, rendered through the ONE `RemoteConnectView`); a door's queued
 * arrival lands once the list does (mount that share, or put the cursor on the one `..`
 * came out of); a sign-in refusal on a listing the person started opens the sheet on its
 * own, once, and a cancel leaves the pane on the error screen's Sign in; and the error
 * screen names the server by its friendly label. A row leading off the `network` volume
 * (the Servers list's places, a saved share's place) opens there first (`network-row-open.ts`).
 *
 * A factory, ❌ not FilePane code (FilePane is at its size cap), and ❌ never a view
 * branch: the views render the share list as any listing (`../views/list-words.ts`).
 *
 * ❗ The ask fires only for an arrival a door queued (`share-list-arrival.svelte.ts`) or a
 * listing the pane was asked to run (`noteListingStarted`): a tab coming back into view, a
 * retained listing repainting, or a re-list nobody started finds none and asks nothing
 * (servers principle 4, `#lib/servers/DETAILS.md`).
 */

import type { FileEntry, FriendlyError, NetworkHost, ShareInfo } from '../types'
import type { ShareParentLanding } from '../network/share-parent-row.svelte'
import { findFileIndex, getFileAt, saveS3AccountRoot } from '#lib/tauri-commands/index.js'
import { isShareListLocation, sameShareList } from '#lib/servers/share-list-path.js'
import { untrack } from 'svelte'
import { addToast } from '#lib/ui/toast/index.js'
import { tString } from '#lib/intl/messages.svelte.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { noteShareListListed, shareListServerLabel, shareListServerOf } from '../network/share-list-label'
import { askToSignInToList } from '../network/share-list-sign-in'
import { showShareParentHintOnce } from '../network/share-parent-hint'
import { createShareMount, type ShareMount } from './share-mount.svelte'
import {
  claimShareListAsk,
  dropShareListArrivalOff,
  openShareListArrival,
  queueShareListArrival,
  shareListArrivalFor,
  takeShareListArrival,
} from './share-list-arrival.svelte'
import type { VolumeChangePayload } from './types'
import { capabilitiesForPane, type VolumeKind } from './volume-capabilities'
import { openNetworkRow, type NetworkRowOpenDeps } from './network-row-open'
import { getVolumes } from '#lib/stores/volume-store.svelte.js'

/** The kinds on the `network` volume: the Servers list and a server's share list. */
const NETWORK_KINDS: ReadonlySet<VolumeKind> = new Set(['network', 'share-list'])

const log = getAppLogger('fileExplorer')

export interface ShareListPaneDeps {
  getPaneId: () => 'left' | 'right'
  getVolumeId: () => string
  getCurrentPath: () => string
  getListingId: () => string
  getIncludeHidden: () => boolean
  getFriendlyError: () => FriendlyError | null
  /** Whether the pane shows an error screen in place of the listing (typed or not). */
  getListingFailed: () => boolean
  /** Lands the cursor on a row (`..` is row 0) and scrolls it into view. */
  moveCursorTo: (index: number) => void
  /** Lists the share list again, the way Retry does. */
  relist: () => void
  /**
   * Puts keyboard focus on the in-pane sign-in state's Sign in, where a cancelled ask leaves
   * the pane: the error screen's (`ErrorPane`) for a refused listing, the mount's
   * (`RemoteConnectView`) for a refused mount. Enter asks again.
   */
  focusSignIn: () => void
  /** Takes the pane to another volume (the mounted share) or to a share list. */
  onVolumeChange: (change: VolumeChangePayload) => void
  /** Pushes the pane's state to `cmdr://state`. */
  syncMcp: () => void
}

export interface ShareListPane {
  /** Mounting a share: `view` for `RemoteConnectView`, its keys, and its `mountError`. */
  readonly mount: ShareMount
  /** The server's friendly name on a share list (what the error screen shows), else `undefined`. */
  readonly label: string | undefined
  /**
   * Enter, a double-click, or MCP `open_under_cursor` on `entry`: a row leading off the
   * `network` volume goes there (`network-row-open.ts`: a Servers row's place, an S3
   * account's root, a saved share's place), else a share on a share list mounts. Answers
   * whether it took the entry (`..`, an SMB server's row, and any other pane's rows it leaves).
   */
  openShare: (entry: FileEntry) => boolean
  /** A door in this pane opening `host`'s share list (the hub's rows, `..` out of a share). */
  openServer: (host: NetworkHost, arrival?: { mount?: string; land?: ShareParentLanding }) => void
  /** The pane was asked to list `path` (a navigation, Retry, an account door): a share list's refusal may ask. */
  noteListingStarted: (path: string) => void
  /**
   * The pane committed `path` from a listing: spends the arrival waiting for it, once the
   * rows are on screen. ❗ An error screen commits its path too, and spends nothing: a
   * sign-in refusal keeps the share to mount for the re-list the sign-in runs.
   */
  landed: (path: string) => void
}

export function createShareListPane(deps: ShareListPaneDeps): ShareListPane {
  const onShareList = $derived(isShareListLocation(deps.getVolumeId(), deps.getCurrentPath()))
  const onNetworkVolume = $derived(
    NETWORK_KINDS.has(capabilitiesForPane(deps.getVolumeId(), deps.getCurrentPath()).kind),
  )
  const networkRowDeps: NetworkRowOpenDeps = {
    onVolumeChange: (change) => {
      deps.onVolumeChange(change)
    },
    getVolumes: () => getVolumes(),
    saveS3AccountRoot: (accountId) => saveS3AccountRoot(accountId),
    explainNoAccountRoot: (name) => {
      addToast(tString('fileExplorer.network.servers.accountRootGone', { name }), { level: 'info' })
    },
  }

  const mount = createShareMount({
    getServer: () => (onShareList ? shareListServerOf(deps.getCurrentPath()) : null),
    onVolumeChange: (change) => {
      deps.onVolumeChange(change)
    },
    focusSignIn: () => {
      deps.focusSignIn()
    },
  })

  // An arrival is for the visit it was queued with: anywhere else, it's gone.
  $effect(() => {
    dropShareListArrivalOff(deps.getPaneId(), deps.getCurrentPath())
  })

  // A refused mount says so in `cmdr://state`, and stops saying so once it's gone.
  // Not `$state`: writing it inside the effect that reads it would re-run the effect forever.
  let mirroredMountError = false
  $effect(() => {
    const info = mount.mountErrorInfo
    if (info === null && !mirroredMountError) return
    mirroredMountError = info !== null
    untrack(() => {
      deps.syncMcp()
    })
  })

  // A sign-in refusal on a share list the person opened asks for an account, once.
  $effect(() => {
    const friendly = deps.getFriendlyError()
    const path = deps.getCurrentPath()
    if (!onShareList || friendly?.actionKind !== 'sign_in_to_server' || !friendly.shareListRefusal) return
    if (!claimShareListAsk(deps.getPaneId(), path)) return
    void askOnce(path, friendly.shareListRefusal)
  })

  async function askOnce(path: string, refusal: NonNullable<FriendlyError['shareListRefusal']>): Promise<void> {
    const outcome = await askToSignInToList(path, refusal)
    if (!sameShareList(deps.getCurrentPath(), path)) return
    // ❗ Cancelling STAYS on the in-pane sign-in state, focus on its Sign in: that screen is the
    // honest "you're not signed in" place, and a cancel that left for Servers made it unreachable.
    if (outcome?.kind === 'cancelled') deps.focusSignIn()
    else if (outcome) deps.relist()
  }

  /** The share on row `name` of the landed listing, as the mount takes it, or `null`. */
  async function shareRow(name: string): Promise<{ index: number; share: ShareInfo } | null> {
    const listingId = deps.getListingId()
    const includeHidden = deps.getIncludeHidden()
    const index = await findFileIndex(listingId, name, includeHidden)
    if (index === null) return null
    const entry = await getFileAt(listingId, index, includeHidden)
    if (!entry) return null
    return { index, share: shareOf(entry) }
  }

  async function arrive(path: string): Promise<void> {
    const arrival = takeShareListArrival(deps.getPaneId(), path)
    const shareName = arrival?.mount ?? arrival?.land?.shareName
    if (!arrival || shareName === undefined) return
    const row = await shareRow(shareName).catch((error: unknown) => {
      log.warn('Finding {share} in the share list did not go through: {error}', { share: shareName, error })
      return null
    })
    if (!sameShareList(deps.getCurrentPath(), path)) return
    if (arrival.land) {
      // `..` is row 0, so a share's row is one below its index in the listing.
      if (row) deps.moveCursorTo(row.index + 1)
      showShareParentHintOnce(arrival.land)
      return
    }
    if (row) {
      void mount.mount(row.share, null)
      return
    }
    addToast(
      tString('fileExplorer.network.share.notFound', {
        shareName,
        hostName: shareListServerLabel(path) ?? path,
      }),
      { level: 'warn' },
    )
  }

  return {
    mount,
    get label() {
      return onShareList ? (shareListServerLabel(deps.getCurrentPath()) ?? undefined) : undefined
    },
    openShare: (entry) => {
      if (entry.name === '..') return false
      // A row leading off the `network` volume (a Servers row's place, a saved share's place) goes there first.
      if (onNetworkVolume && openNetworkRow(entry, networkRowDeps)) return true
      // An SMB server's Servers row is a door: the person opens that server, so its arrival is queued (a refused
      // listing asks once), and the generic folder open takes the pane to the row's path.
      if (onNetworkVolume && !onShareList && isShareListLocation(deps.getVolumeId(), entry.path)) {
        queueShareListArrival(deps.getPaneId(), { path: entry.path, origin: deps.getCurrentPath() })
        return false
      }
      if (!onShareList) return false
      // ❗ One mount at a time: Enter or MCP on a row the wait still covers starts nothing.
      if (mount.view === null) void mount.mount(shareOf(entry), null)
      return true
    },
    openServer: (host, arrival = {}) => {
      const targetPath = openShareListArrival(deps.getPaneId(), host, { origin: deps.getCurrentPath(), ...arrival })
      deps.onVolumeChange({ volumeId: 'network', volumePath: 'smb://', targetPath, exact: true })
    },
    noteListingStarted: (path) => {
      if (!isShareListLocation(deps.getVolumeId(), path)) return
      const pane = deps.getPaneId()
      // A door's own arrival keeps its share; either way this listing may ask again.
      const queued = shareListArrivalFor(pane)
      const keep = queued !== null && sameShareList(queued.path, path) ? queued : { origin: path }
      queueShareListArrival(pane, { ...keep, path, asked: false })
    },
    landed: (path) => {
      if (!isShareListLocation(deps.getVolumeId(), path) || deps.getListingFailed()) return
      noteShareListListed(path)
      void arrive(path)
    },
  }
}

/** A share row as the mount takes it: its name, and the description the server set. */
function shareOf(entry: FileEntry): ShareInfo {
  return { name: entry.name, isDisk: true, comment: entry.shareMeta?.description ?? null }
}
