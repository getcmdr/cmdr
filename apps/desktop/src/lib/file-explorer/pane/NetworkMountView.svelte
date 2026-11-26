<script lang="ts">
    /**
     * The Servers hub on the `network` volume at `smb://`. A server's share list is the
     * same volume at `smb://<server>`, a real listing the pane renders like any folder
     * (`share-list-pane.svelte.ts`), so every way this view opens one goes through the
     * pane's `onOpenServer`: ❌ this view holds no host.
     */
    import { tick, untrack } from 'svelte'
    import { openAddServerSheet } from '#lib/servers/open-sign-in.js'
    import { focusHome } from '#lib/ui/focus-restore.js'
    import type { NetworkHost } from '../types'
    import { getVolumes } from '#lib/stores/volume-store.svelte.js'
    import { pathForPickedVolume } from '../navigation/picked-volume-path'
    import { getAppLogger } from '#lib/logging/logger.js'
    import type { ServersHubAPI, NetworkCursorEntry } from './types'
    import ServersHub from '../network/ServersHub.svelte'
    import type { HubRow } from '../network/servers-hub-rows'
    import { holdDiscoveryForServersView } from '../network/network-store.svelte'
    import type { ServerProtocol } from '#lib/ipc/bindings.js'
    import { hubLandingFor, takeHubLanding } from './hub-landing.svelte'
    import type { VolumeChangePayload } from './types'

    const log = getAppLogger('fileExplorer')

    interface Props {
        paneId?: 'left' | 'right'
        isFocused?: boolean
        onVolumeChange?: (change: VolumeChangePayload) => void
        /** Opens `host`'s share list in this pane, mounting `mount` once it lands when named. */
        onOpenServer?: (host: NetworkHost, arrival: { mount?: string }) => void
    }

    const { paneId, isFocused = false, onVolumeChange, onOpenServer }: Props = $props()

    // Hosts arrive and leave live while this view is on screen: the backend browses mDNS
    // exactly as long as some Servers view holds it. The release is the effect's cleanup.
    $effect(() => holdDiscoveryForServersView())

    let serversHubRef: ServersHubAPI | undefined = $state()

    // The server the pane came back from (its share list's `..`, or a restored share-list
    // tab): the hub's cursor lands on its row, once (`hub-landing.svelte.ts`).
    $effect(() => {
        const hub = serversHubRef
        if (!paneId || !hub || hubLandingFor(paneId) === undefined) return
        untrack(() => {
            const landing = takeHubLanding(paneId)
            if (landing !== undefined) hub.selectShareListServer(landing)
        })
    })

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
     * The Add row opens the ONE sign-in sheet (the NFS hint opens it on NFS). An SMB
     * address lands back here as a hand-off: its connect is a share MOUNT rather than a
     * session, so the injected host's share list opens, mounting the share the address named.
     */
    async function openAddServer(protocol?: ServerProtocol) {
        await openAddServerSheet({
            protocol,
            onSmbHandOff: (handOff) => {
                onOpenServer?.(handOff.host, { mount: handOff.sharePath ?? undefined })
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

    export function handleKeyDown(e: KeyboardEvent) {
        serversHubRef?.handleKeyDown(e)
    }

    /** Move cursor to a specific index (used by MCP move_cursor tool). */
    export function setCursorIndex(index: number) {
        serversHubRef?.setCursorIndex(index)
    }

    /** Rows the cursor can sit on right now. */
    export function getItemCount(): number {
        return serversHubRef?.getItemCount() ?? 0
    }

    /** Type-to-jump in the hub; answers the name it landed on, or `null`. */
    export function jumpToPrefix(prefix: string): string | null {
        return serversHubRef?.jumpToPrefix(prefix) ?? null
    }

    /** Find an item by name, returns its index or -1. */
    export function findItemIndex(name: string): number {
        return serversHubRef?.findItemIndex(name) ?? -1
    }

    /** What's under the hub's cursor: an SMB host, another server's row, or `null` (the Add row, an empty hub). */
    // noinspection JSUnusedGlobalSymbols -- used by FilePane.getNetworkCursorEntry
    export function getNetworkCursorEntry(): NetworkCursorEntry | null {
        const row = serversHubRef?.getRowUnderCursor() ?? null
        const host = serversHubRef?.getHostUnderCursor() ?? null
        if (host && row) return { kind: 'host', host, row }
        return row ? { kind: 'server', row } : null
    }

    /** Opens the row under the cursor, the same action Enter triggers. */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by FilePane / MCP
    export function openCursorItem(): void {
        serversHubRef?.openCursorItem()
    }

    /** ⌃⏎: the servers hub's cursor row menu. */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by FilePane
    export async function openContextMenuAtCursor(): Promise<void> {
        await serversHubRef?.openContextMenuAtCursor()
    }

    /** Refresh network hosts (used by ⌘R shortcut). */
    export function refreshNetworkHosts() {
        serversHubRef?.refresh()
    }
</script>

<!-- A host, "More shares on…", and a saved share no mount went through all open the server's
     share list in this pane; the last one mounts that share once it lands. -->
<ServersHub
    bind:this={serversHubRef}
    {paneId}
    {isFocused}
    onHostSelect={(host: NetworkHost) => onOpenServer?.(host, {})}
    onServerSelect={handleServerSelect}
    onShareViaHost={(host: NetworkHost, { share }: { share: string; label: string }) => onOpenServer?.(host, { mount: share })}
    onConnectToServer={(protocol?: ServerProtocol) => void openAddServer(protocol)}
/>
