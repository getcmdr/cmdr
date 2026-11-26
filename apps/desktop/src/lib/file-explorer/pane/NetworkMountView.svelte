<script lang="ts">
    import { tick, untrack } from 'svelte'
    import { openAddServerSheet } from '#lib/servers/open-sign-in.js'
    import { focusHome } from '#lib/ui/focus-restore.js'
    import type { NetworkHost, PlacesAccount, ShareInfo } from '../types'
    import { getVolumes } from '#lib/stores/volume-store.svelte.js'
    import { pathForPickedVolume } from '../navigation/picked-volume-path'
    import { getAppLogger } from '#lib/logging/logger.js'
    import type { ServersHubAPI, PlacesBrowserAPI, NetworkCursorEntry } from './types'
    import ServersHub from '../network/ServersHub.svelte'
    import type { HubRow } from '../network/servers-hub-rows'
    import PlacesBrowser from '../network/PlacesBrowser.svelte'
    import { holdDiscoveryForServersView } from '../network/network-store.svelte'
    import type { ServerProtocol } from '#lib/ipc/bindings.js'
    import RemoteConnectView from './RemoteConnectView.svelte'
    import { createShareMount, type ShareMountCredentials } from './share-mount.svelte'
    import type { VolumeChangePayload } from './types'
    import type { ShareParentLanding } from '../network/share-parent-row.svelte'

    const log = getAppLogger('fileExplorer')

    interface Props {
        paneId?: 'left' | 'right'
        isFocused?: boolean
        /** Externally controlled network host (for history navigation) */
        initialNetworkHost?: NetworkHost | null
        /**
         * Share name to auto-mount on this host. Used by "Copy path between
         * panes" when the source pane has the cursor on a share. Reactive:
         * a new non-empty value re-arms the auto-mount gate.
         */
        initialAutoMountShare?: string | undefined
        /**
         * The queued share is spent: it was handed to a mount, or the visit it was
         * queued with ended. The parent clears its copy, so the same share name can be
         * queued again later.
         */
        onAutoMountConsumed?: () => void
        /**
         * The share `..` came out of (#1238): its host's share list puts the cursor on it,
         * and reports back through `onShareLanded` once the list is on screen.
         */
        shareLanding?: ShareParentLanding
        onShareLanded?: () => void
        onVolumeChange?: (change: VolumeChangePayload) => void
        onNetworkHostChange?: (host: NetworkHost | null) => void
    }

    const {
        paneId,
        isFocused = false,
        initialNetworkHost = null,
        initialAutoMountShare,
        onAutoMountConsumed,
        shareLanding,
        onShareLanded,
        onVolumeChange,
        onNetworkHostChange,
    }: Props = $props()

    // Two-way synced state: parent drives it via initialNetworkHost (history nav),
    // child drives it locally (host selection, mount success, back). Can't be $derived.
    // eslint-disable-next-line svelte/prefer-writable-derived -- bidirectional sync with local overrides
    let currentNetworkHost = $state<NetworkHost | null>(initialNetworkHost)
    /** What the hub called the open host (a name the person gave it), for the share list's words. */
    let currentHostLabel = $state<string | undefined>(undefined)

    /** The open host as an account, which is what `PlacesBrowser` takes. */
    const currentAccount = $derived<PlacesAccount | null>(
        currentNetworkHost ? { protocol: 'smb', host: currentNetworkHost, label: currentHostLabel } : null,
    )

    /** Mounting a share: the wait, the sign-in a refusal asks for, the refusal, and the queued share. */
    const shareMount = createShareMount({
        getPaneId: () => paneId,
        getServer: () => (currentNetworkHost ? { host: currentNetworkHost, label: currentHostLabel } : null),
        onMounted: () => {
            // Propagated up too, so the parent doesn't hold onto a stale host: otherwise the
            // next visit to Network would open this host's share list instead of the hub.
            currentNetworkHost = null
            onNetworkHostChange?.(null)
        },
        onVolumeChange: (change) => onVolumeChange?.(change),
        onPendingConsumed: () => onAutoMountConsumed?.(),
    })

    // Hosts arrive and leave live while this view is on screen: the backend browses mDNS
    // exactly as long as some Servers view holds it. The release is the effect's cleanup.
    $effect(() => holdDiscoveryForServersView())

    // Component refs for keyboard navigation
    let serversHubRef: ServersHubAPI | undefined = $state()
    let placesBrowserRef: PlacesBrowserAPI | undefined = $state()

    // Sync when parent changes the prop (for example, history navigation)
    $effect(() => {
        currentNetworkHost = initialNetworkHost
    })

    // The parent's queued share ("Copy path between panes" with the cursor on a share).
    $effect(() => {
        const queued = initialAutoMountShare
        untrack(() => {
            shareMount.adoptQueued(queued)
        })
    })

    /**
     * A host picked from the servers list: its shares, ❌ never a share mounted
     * on the person's behalf. They picked a HOST.
     */
    function handleNetworkHostSelect(host: NetworkHost, label?: string) {
        shareMount.retirePending()
        currentHostLabel = label
        currentNetworkHost = host
        onNetworkHostChange?.(host)
    }

    /**
     * Enter on a one-place server in the hub: take the pane to its place, a saved
     * one on its start folder (`pathForPickedVolume`).
     *
     * ❗ The pane does the dialing, not the hub. Landing on a `saved` volume is
     * what `place-connect` watches for, so the connecting view and its Cancel
     * render where every other wait does.
     */
    function handleServerSelect(row: HubRow) {
        const volume = getVolumes().find((v) => v.id === row.volumeId)
        const root = volume?.path ?? row.place?.appRoot ?? row.saved?.places[0]?.appRoot
        if (!row.volumeId || !root) {
            log.warn('The hub row {server} has no place to open', { server: row.name })
            return
        }
        const targetPath = volume ? pathForPickedVolume(volume) : root
        onVolumeChange?.({ volumeId: row.volumeId, volumePath: root, targetPath })
    }

    /**
     * The Add row opens the ONE sign-in sheet (the NFS hint opens it on NFS). An SMB address lands back here as
     * a hand-off: its connect is a share MOUNT rather than a session, so the
     * host is injected and this view opens its places list.
     */
    async function openAddServer(protocol?: ServerProtocol) {
        await openAddServerSheet({
            protocol,
            onSmbHandOff: (handOff) => {
                shareMount.retirePending()
                currentNetworkHost = handOff.host
                onNetworkHostChange?.(handOff.host)
                // The one share the ADDRESS named, for this visit only.
                if (handOff.sharePath) shareMount.queue(handOff.sharePath)
            },
            // The hub's own pane goes to the place it just added, the way picking
            // its row would take it there.
            onConnected: ({ volumeId, root }) => {
                onVolumeChange?.({ volumeId, volumePath: root, targetPath: root })
            },
            // "Add" saved it without opening: the new row, selected, is the proof.
            onAdded: ({ serverId }) => {
                serversHubRef?.selectServer(serverId)
            },
        })
        await tick()
        // Focus goes back to the explorer so keyboard navigation resumes.
        focusHome()
    }

    /**
     * A saved share no mount went through yet: its host's share list, mounting
     * that one share for this visit (the account comes from the host's typed
     * one, which keeps the listing from answering as guest).
     */
    function handleShareViaHost(host: NetworkHost, { share, label }: { share: string; label: string }) {
        handleNetworkHostSelect(host, label)
        shareMount.queue(share)
    }

    /** Enter on a share: mount it, with the account the listing signed in as when it has one. */
    function handleShareSelect(share: ShareInfo, credentials: ShareMountCredentials | null) {
        void shareMount.mount(share, credentials)
    }

    function handleNetworkBack() {
        const cameFrom = currentNetworkHost
        shareMount.retirePending()
        currentNetworkHost = null
        shareMount.reset()
        onNetworkHostChange?.(null)
        // The cursor goes back to the host the person came from, by its identity (a saved
        // row carries the host's id, a discovered one its `host`), ❌ not the first row.
        if (cameFrom) void tick().then(() => serversHubRef?.selectServer(cameFrom.id))
    }

    export function handleKeyDown(e: KeyboardEvent) {
        // A mount running, its refusal, or its sign-in sheet owns the keyboard.
        if (shareMount.handleKeyDown(e)) return
        if (currentNetworkHost) {
            placesBrowserRef?.handleKeyDown(e)
        } else {
            serversHubRef?.handleKeyDown(e)
        }
    }

    /** Move cursor to a specific index (used by MCP move_cursor tool). */
    export function setCursorIndex(index: number) {
        if (currentNetworkHost) {
            placesBrowserRef?.setCursorIndex(index)
        } else {
            serversHubRef?.setCursorIndex(index)
        }
    }

    /**
     * Rows the cursor can sit on right now. `0` while a mount runs or its refusal is
     * on screen: neither browser is mounted then, so a cursor move would land nowhere
     * and the MCP tool would report a move that never happened.
     */
    export function getItemCount(): number {
        if (shareMount.view) return 0
        if (currentNetworkHost) return placesBrowserRef?.getItemCount() ?? 0
        return serversHubRef?.getItemCount() ?? 0
    }

    /** Type-to-jump in whichever browser is up; answers the name it landed on, or `null`. */
    export function jumpToPrefix(prefix: string): string | null {
        if (shareMount.view) return null
        return (currentNetworkHost ? placesBrowserRef : serversHubRef)?.jumpToPrefix(prefix) ?? null
    }

    /** Find an item by name, returns its index or -1. */
    export function findItemIndex(name: string): number {
        if (currentNetworkHost) {
            return placesBrowserRef?.findItemIndex(name) ?? -1
        }
        return serversHubRef?.findItemIndex(name) ?? -1
    }

    /**
     * Returns what's under the cursor in the network browser stack:
     * a host (server list), a share (share list), or `null` (connect row,
     * login form, mounting state, mount refusal, empty list).
     */
    // noinspection JSUnusedGlobalSymbols -- used by FilePane.getNetworkCursorEntry
    export function getNetworkCursorEntry(): NetworkCursorEntry | null {
        if (shareMount.view) return null
        if (currentNetworkHost) {
            const share = placesBrowserRef?.getShareUnderCursor() ?? null
            return share ? { kind: 'share', share } : null
        }
        const row = serversHubRef?.getRowUnderCursor() ?? null
        const host = serversHubRef?.getHostUnderCursor() ?? null
        if (host && row) return { kind: 'host', host, row }
        return row ? { kind: 'server', row } : null
    }

    /** Opens the host or share under the cursor — same action Enter triggers. */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by FilePane / MCP
    export function openCursorItem(): void {
        if (currentNetworkHost) {
            placesBrowserRef?.openCursorItem()
        } else {
            serversHubRef?.openCursorItem()
        }
    }

    /** ⌃⏎: the servers hub's cursor row menu. A host's places list has no row menu, so nothing there. */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by FilePane
    export async function openContextMenuAtCursor(): Promise<void> {
        if (currentNetworkHost) return
        await serversHubRef?.openContextMenuAtCursor()
    }

    /** Refresh network hosts (used by ⌘R shortcut). */
    export function refreshNetworkHosts() {
        serversHubRef?.refresh()
    }

    export function setNetworkHost(host: NetworkHost | null) {
        if (host?.id !== currentNetworkHost?.id) {
            shareMount.retirePending()
            currentHostLabel = undefined
        }
        currentNetworkHost = host
        shareMount.reset()
    }
</script>

{#if shareMount.view}
    <!-- ❗ The shared remote-connect view, ❌ not a one-off pane: its Cancel calls off
         the mount, which can hang behind a stuck macOS sign-in helper for as long as
         the timeout allows (#604). Keyed on the wait, so a retry's wait starts with the
         view's own quiet reveal again, as a fresh one does. -->
    {#key shareMount.view.state.kind === 'connecting'}
        <RemoteConnectView
            name={shareMount.view.name}
            server={shareMount.view.server}
            address={shareMount.view.address}
            state={shareMount.view.state}
        />
    {/key}
{:else if currentAccount}
    <PlacesBrowser
        bind:this={placesBrowserRef}
        account={currentAccount}
        {paneId}
        {isFocused}
        autoMountShare={shareMount.pendingMount}
        takeAutoMount={shareMount.takePending}
        onShareSelect={handleShareSelect}
        onBack={handleNetworkBack}
        landOnShare={shareLanding?.hostId === currentAccount.host.id ? shareLanding.shareName : undefined}
        onLandedOnShare={() => onShareLanded?.()}
    />
{:else}
    <ServersHub
        bind:this={serversHubRef}
        {paneId}
        {isFocused}
        onHostSelect={handleNetworkHostSelect}
        onServerSelect={handleServerSelect}
        onShareViaHost={handleShareViaHost}
        onConnectToServer={(protocol?: ServerProtocol) => void openAddServer(protocol)}
    />
{/if}
