/**
 * The servers family: showing the hub, and the actions that mirror a server row's
 * context menu for people who reach for the keyboard.
 *
 * ❗ **Every action arm goes through the function its row menu runs**: a place's
 * through `runServerRowAction`, an SMB server's account actions through
 * `runServerAccountAction`, and its Disconnect through `disconnectSmbHost`. One
 * place names what each costs, so a menu item and a palette command can't drift on
 * the confirmation or the toast.
 *
 * Which server they act on is `serverCommandTarget`'s call: the hub's cursor row
 * wherever there is one, else the server whose share list the pane shows, else the
 * focused pane's own volume. A command that finds none says nothing — the palette
 * lists every command whatever the pane is on, and a toast for "you weren't
 * pointing at a server" is noise.
 */
import { runServerRowAction } from '#lib/file-explorer/navigation/server-row-actions.js'
import { openAddServerSheet } from '#lib/servers/open-sign-in.js'
import {
  createServerRowActions,
  disconnectSmbHost,
  editServerInView,
} from '#lib/file-explorer/network/servers-list-actions.js'
import { runServerAccountAction } from '#lib/file-explorer/network/share-list-sign-in.js'
import type { ServerAccountAction } from '#lib/file-explorer/navigation/row-menu.js'
import type { NetworkHost } from '#lib/file-explorer/types.js'
import { isSmbShareVolume, toggleShareParentRow } from '#lib/file-explorer/network/share-parent-row.svelte.js'
import { serverCommandTarget, type ServerCommandTarget } from '#lib/servers/server-command-target.js'
import { getFocusedPanePath, getFocusedPaneVolumeId } from '#lib/file-explorer/pane/focused-pane-reads.js'
import { shareListHostAt } from '#lib/file-explorer/network/share-list-label.js'
import { capabilitiesForPane } from '#lib/file-explorer/pane/volume-capabilities.js'
import { getVolumes } from '#lib/stores/volume-store.svelte.js'
import { listSavedServers } from '#lib/tauri-commands/index.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { addToast } from '#lib/ui/toast/index.js'
import { tString } from '#lib/intl/messages.svelte.js'
import type { ExplorerAPI } from '../explorer-api'
import { detached } from './detached'
import type { CommandHandlerRecord } from './types'

const log = getAppLogger('servers')

/**
 * The server whose share list the focused pane shows, or `null`. The ONE read of it the
 * servers commands make, so Edit and the target below can't disagree on it.
 */
function shareListHost(): NetworkHost | null {
  return shareListHostAt(getFocusedPaneVolumeId(), getFocusedPanePath())
}

/** The focused pane's path when it shows a server's share list, else `null`. */
function shareListPathInView(): string | null {
  const path = getFocusedPanePath()
  return capabilitiesForPane(getFocusedPaneVolumeId(), path).kind === 'share-list' ? path : null
}

/** The server the user is pointing at, or `null`. */
async function target(explorerRef: ExplorerAPI | undefined): Promise<ServerCommandTarget | null> {
  const volumeId = getFocusedPaneVolumeId()
  return serverCommandTarget({
    cursorRow: (await explorerRef?.getFocusedPaneServerRow()) ?? null,
    shareListHost: shareListHost(),
    paneVolume: getVolumes().find((volume) => volume.id === volumeId) ?? null,
  })
}

/** A place target, for the commands only a place has (the pin). */
type PlaceTarget = Extract<ServerCommandTarget, { kind: 'place' }>

/**
 * An SMB server's account action from the palette, run where its row menu runs it. One
 * that lists from a hub row opens the server's share list, as the row menu's does; on the
 * share list itself the list follows the listing.
 */
async function runAccountAction(explorerRef: ExplorerAPI | undefined, action: ServerAccountAction): Promise<void> {
  const server = await target(explorerRef)
  if (server?.kind !== 'smb-server') return
  const { listed } = await runServerAccountAction(action, { host: server.host, label: server.name })
  if (listed && server.row) explorerRef?.openSmbHandOffInFocusedPane({ host: server.host, sharePath: null })
}

/**
 * Whether the target is pinned right now.
 *
 * A target resolved from the pane's volume can't say (a `VolumeInfo` carries no
 * pin), so the saved list answers. A store that doesn't answer reads as unpinned,
 * which makes the command a pin rather than a silent no-op.
 */
async function isPinned(server: PlaceTarget): Promise<boolean> {
  if (server.pinned !== null) return server.pinned
  try {
    const saved = await listSavedServers()
    return saved.some((entry) => entry.places.some((place) => place.volumeId === server.volumeId && place.pinned))
  } catch (e) {
    log.warn('Reading the pin for {volumeId} broke down: {error}', { volumeId: server.volumeId, error: e })
    return false
  }
}

export const serversHandlers = {
  'servers.show': ({ explorerRef }) => {
    explorerRef?.showServersInFocusedPane()
  },

  'servers.togglePin': async ({ explorerRef }) => {
    // An SMB server has no switcher row to pin.
    const server = await target(explorerRef)
    if (server?.kind !== 'place') return
    const pinned = await isPinned(server)
    await runServerRowAction({
      action: pinned ? 'unpin' : 'pin',
      volumeId: server.volumeId,
      volumeName: server.name,
    })
  },

  'servers.disconnect': async ({ explorerRef }) => {
    const server = await target(explorerRef)
    if (!server) return
    if (server.kind === 'smb-server') {
      await disconnectSmbHost(server.host, server.name)
      return
    }
    await runServerRowAction({ action: 'disconnect', volumeId: server.volumeId, volumeName: server.name })
  },

  'servers.forgetSecret': async ({ explorerRef }) => {
    const server = await target(explorerRef)
    if (!server) return
    if (server.kind === 'smb-server') {
      await runServerAccountAction('forget-secret', { host: server.host, label: server.name })
      return
    }
    await runServerRowAction({ action: 'forget-secret', volumeId: server.volumeId, volumeName: server.name })
  },

  // An SMB server's account, the items its row menu and the share list's account menu carry.
  // ❗ They await the sheet, unlike `servers.edit`: a listing that lands has to open the
  // share list. ❗ So they run detached, or the dispatch would hold the pipeline while the
  // person types.
  'servers.signInAs': ({ explorerRef }) => {
    detached(runAccountAction(explorerRef, 'sign-in-as'))
  },

  'servers.browseAsGuest': ({ explorerRef }) => {
    detached(runAccountAction(explorerRef, 'browse-as-guest'))
  },

  // The focused pane's SMB share, the switch its row menu carries. The palette offers it only
  // there (`focusedPaneOnSmbShare`); from anywhere else (a rebound key) it says nothing.
  'servers.toggleShareParentRow': async () => {
    const volumeId = getFocusedPaneVolumeId()
    if (!isSmbShareVolume(getVolumes().find((volume) => volume.id === volumeId))) return
    await toggleShareParentRow(volumeId)
  },

  // ❗ The two sheet commands OPEN and return, ❌ never await the sheet: a
  // dispatch that doesn't settle until the user is done typing holds the command
  // pipeline open for as long as they take, and nothing downstream reads the
  // answer. `servers.show` behaves the same way.
  'servers.edit': ({ explorerRef }) => {
    // On the Servers volume, the server in view, an SMB host's included: `target` stops
    // at a host, which has no place for the other commands to act on but does have a
    // name to edit.
    if (explorerRef && getFocusedPaneVolumeId() === 'network') {
      const explorer = explorerRef
      detached(
        explorer
          .getFocusedPaneServerRow()
          .then((row) => editServerInView({ row, shareListPath: shareListPathInView() })),
      )
      return
    }
    detached(editPlaceInView(explorerRef))
  },

  'servers.connect': ({ explorerRef }) => {
    detached(openAddServer(explorerRef))
  },
} satisfies Partial<CommandHandlerRecord>

/**
 * F8 on the Servers list (`file.delete` there, the F-key bar's "Forget"): forgets the server
 * under the cursor, per row kind, through the row's own Forget (`createServerRowActions`), keyed
 * by the row's id. A row with no server (nothing under the cursor) does nothing.
 */
export async function forgetServerInView(explorerRef: ExplorerAPI | undefined): Promise<void> {
  const row = (await explorerRef?.getFocusedPaneServerRow()) ?? null
  if (!row) return
  const actions = createServerRowActions({
    getRows: () => [row],
    getVolumes,
    // The `volumes-changed` every Forget emits re-lists the Servers list (`servers_relist.rs`).
    refreshSaved: () => Promise.resolve(),
    openRow: () => {},
  })
  await actions.forget(row)
}

/** `servers.edit` off the Servers volume: the place the palette's target names, if any. */
async function editPlaceInView(explorerRef: ExplorerAPI | undefined): Promise<void> {
  const server = await target(explorerRef)
  if (server?.kind !== 'place') return
  await runServerRowAction({ action: 'edit', volumeId: server.volumeId, volumeName: server.name })
}

/**
 * The Add server sheet: ⌘K (`servers.connect`, Finder's binding for the same thing) and F7 on
 * the Servers list (`file.newFolder` there). ❗ It opens the sheet directly rather than through
 * `serverCommandTarget`: adding a server is about no server in particular, so what the pane is
 * pointing at is irrelevant. ❗ Resolves when the sheet closes, so callers run it `detached`.
 */
export async function openAddServer(explorerRef: ExplorerAPI | undefined): Promise<void> {
  await openAddServerSheet({
    // An SMB address opens the host it saved, as its share list, mounting the
    // share the address named: its connect is a share MOUNT, not a session.
    onSmbHandOff: (handOff) => {
      explorerRef?.openSmbHandOffInFocusedPane(handOff)
    },
    // An SFTP, WebDAV, or S3 server is a place, so the focused pane goes there: a sheet
    // that closed on a live server with every pane where it was reads as a Connect
    // that did nothing. The root lands on the place's start folder.
    onConnected: ({ volumeId, root }) => {
      if (!explorerRef) return
      explorerRef.navigate({
        pane: explorerRef.getFocusedPane(),
        to: { selectVolume: { volumeId, path: root } },
        source: 'user',
      })
    },
    // "Add" saved it without opening: on the Servers list the new row, selected, is the proof;
    // anywhere else a toast says where it went.
    onAdded: ({ serverId, name }) => {
      if (explorerRef?.selectAddedServerInFocusedPane(serverId)) return
      addToast(tString('servers.sheet.addedToast', { name }), { level: 'success' })
    },
  })
}
