<script lang="ts">
    /**
     * A servers hub row's right-click (and ⌃⏎) menu: the house `Menu` at the point it's
     * given, holding the same list the volume switcher row's → submenu shows for a one-place
     * row, and the host builder's for an SMB host (`../navigation/row-menu.ts`), so no two
     * surfaces can drift. WHICH entries and what a pick does are `servers-hub-actions.ts`'s;
     * this is the surface.
     *
     * ❗ Opening reads no password: the account items come from what this session's listings
     * said, and Forget saved password is offered unconditionally (`row-menu.ts`). The one
     * question it asks on open is the mount table's (does Disconnect do anything?).
     */
    import { onDestroy } from 'svelte'
    import Menu from '#lib/ui/Menu.svelte'
    import { createMenu } from '#lib/ui/menu-controller.svelte.js'
    import { captureFocusOrigin, restoreFocus, type FocusOrigin } from '#lib/ui/focus-restore.js'
    import { smbHostHasMounts } from '#lib/tauri-commands/index.js'
    import { rowMenuSections, type ServerRowMenuEntry } from '../navigation/row-menu'
    import { smbHostOf, type HubActions, type RowMenuOpenFacts } from './servers-hub-actions'
    import type { HubRow } from './servers-hub-rows'
    import type { MenuAnchor } from '#lib/tauri-commands/file-actions.js'

    interface Props {
        actions: HubActions
    }

    const { actions }: Props = $props()

    /**
     * The id of the row whose menu is up, and where focus was when it opened. ❗ The
     * id, ❌ never the row object: a Pin leaves the menu up, and a held row kept
     * offering the pin it had just flipped. The row is read live by id.
     */
    let menuRowId = $state<string | null>(null)
    let opened = $state<RowMenuOpenFacts>({ hasMounts: false })
    let focusBeforeOpen: FocusOrigin | null = null
    const menuRow = $derived(menuRowId === null ? null : actions.rowById(menuRowId))

    // Read live, so a transfer starting under the open menu greys its Disconnect.
    const menu = createMenu<ServerRowMenuEntry>({
        getSections: () => {
            const row = menuRow
            const rowMenu = row ? actions.rowMenu(row, opened) : null
            return row && rowMenu ? rowMenuSections(row.id, rowMenu, (entry) => entry) : []
        },
        onSelect: (item) => {
            if (menuRow && item.data) void actions.runRowEntry(menuRow, item.data)
        },
        restoreFocus: () => {
            restoreFocus(focusBeforeOpen)
            focusBeforeOpen = null
        },
    })

    /**
     * Opens `row`'s menu at `point`: the pointer for a right-click, the spot just under the
     * row for ⌃⏎. A row with nothing to offer opens nothing.
     */
    export async function open(row: HubRow, point: MenuAnchor): Promise<void> {
        const host = smbHostOf(row)
        const facts: RowMenuOpenFacts = { hasMounts: host ? await smbHostHasMounts(host).catch(() => false) : false }
        if (!actions.rowMenu(row, facts)) return
        opened = facts
        menuRowId = row.id
        focusBeforeOpen = captureFocusOrigin()
        menu.openAt(point)
    }

    onDestroy(() => {
        menu.destroy()
    })
</script>

<!-- Named for the server it acts on, which is what a screen reader needs to hear. -->
<Menu {menu} ariaLabel={menuRow?.name ?? ''} />
