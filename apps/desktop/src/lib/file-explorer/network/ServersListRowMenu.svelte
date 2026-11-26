<script lang="ts">
    /**
     * A Servers list row's right-click (and ⌃⏎) menu: the same lists the hub's rows carry
     * (`row-menu.ts`: `smbHostRowMenu` for an SMB server, `volumeRowMenu` for a one-place
     * server), run by the same actions (`servers-hub-actions.ts::createHubActions`), so no two
     * surfaces can drift. The row is resolved by its OWN id (`server-row-of-entry.ts`), and
     * re-resolved after a pick, so a Pin that keeps the menu up offers the flipped pin.
     *
     * Headless: the pane mounts it beside the path (where a share list's account chip sits)
     * and hands it each row's menu, ❌ never the native file menu, whose Copy, Delete, and
     * Reveal are all wrong for a server. Its menu is a member of the chip's ONE `openMenu`
     * (`VolumeBreadcrumb.svelte`), reported through `onOpenChange`, so `isHeaderMenuOpen()`
     * stays the single answer the panes suppress keys on.
     *
     * ❗ Opening reads no password: the account items come from what this session's listings
     * said. The one question it asks on open is the mount table's (does Disconnect do anything?).
     */
    import { onDestroy } from 'svelte'
    import Menu from '#lib/ui/Menu.svelte'
    import { createMenu } from '#lib/ui/menu-controller.svelte.js'
    import { captureFocusOrigin, restoreFocus, type FocusOrigin } from '#lib/ui/focus-restore.js'
    import { smbHostHasMounts, type MenuAnchor } from '#lib/tauri-commands/index.js'
    import { getVolumes } from '#lib/stores/volume-store.svelte.js'
    import { rowMenuSections, type ServerRowMenuEntry } from '../navigation/row-menu'
    import { createHubActions, smbHostOf, type RowMenuOpenFacts } from './servers-hub-actions'
    import { readServerRowOf } from './server-row-of-entry'
    import type { HubRow } from './servers-hub-rows'
    import type { FileEntry } from '../types'

    interface Props {
        /** So the chip keeps one header menu open at a time (`VolumeBreadcrumb.noteAccountMenu`). */
        onOpenChange: (change: { open: boolean; close: () => void }) => void
        /** The menu's Open, and an account action that listed: the pane opens the row, as Enter does. */
        onOpenRow: (entry: FileEntry) => void
    }

    const { onOpenChange, onOpenRow }: Props = $props()

    /** The row whose menu is up, as resolved when it opened (or after the last pick), and its entry. */
    let row = $state<HubRow | null>(null)
    let entry: FileEntry | null = null
    let opened = $state<RowMenuOpenFacts>({ hasMounts: false })
    let focusBeforeOpen: FocusOrigin | null = null
    /** Where the last right-click landed, for a row menu the list opens with no anchor of its own. */
    let pointer: MenuAnchor | null = null

    const actions = createHubActions({
        getRows: () => (row ? [row] : []),
        getVolumes,
        refreshSaved: async () => {
            if (entry) row = await readServerRowOf(entry)
        },
        openRow: () => {
            if (entry) onOpenRow(entry)
        },
    })

    // Read live, so a transfer starting under the open menu greys its Disconnect.
    const menu = createMenu<ServerRowMenuEntry>({
        getSections: () => {
            const rowMenu = row ? actions.rowMenu(row, opened) : null
            return row && rowMenu ? rowMenuSections(row.id, rowMenu, (item) => item) : []
        },
        onSelect: (item) => {
            const picked = row
            if (!picked || !item.data) return
            void actions.runRowEntry(picked, item.data).then(async () => {
                if (entry) row = await readServerRowOf(entry)
            })
        },
        onOpenChange: (open) => {
            onOpenChange({ open, close: menu.close })
        },
        restoreFocus: () => {
            restoreFocus(focusBeforeOpen)
            focusBeforeOpen = null
        },
    })

    /**
     * `entry`'s menu, at `anchor` (⌃⏎ passes the cursor row's) or where the right-click landed.
     * A row with no server behind it, or one with nothing to offer, opens nothing.
     */
    export async function openRowMenu(target: FileEntry, anchor: MenuAnchor | null | undefined): Promise<void> {
        const at = anchor ?? pointer
        if (!at) return
        const resolved = await readServerRowOf(target)
        if (!resolved) return
        const host = smbHostOf(resolved)
        const facts: RowMenuOpenFacts = { hasMounts: host ? await smbHostHasMounts(host).catch(() => false) : false }
        if (!actions.rowMenu(resolved, facts)) return
        entry = target
        row = resolved
        opened = facts
        focusBeforeOpen = captureFocusOrigin()
        menu.openAt(at)
    }

    // The list's right-click reaches the pane without its event, so the point is kept here.
    $effect(() => {
        const note = (event: MouseEvent) => {
            pointer = { x: event.clientX, y: event.clientY }
        }
        document.addEventListener('contextmenu', note, true)
        return () => {
            document.removeEventListener('contextmenu', note, true)
        }
    })

    onDestroy(() => {
        menu.destroy()
    })
</script>

<!-- Named for the server it acts on, which is what a screen reader needs to hear. -->
<Menu {menu} ariaLabel={row?.name ?? ''} />
