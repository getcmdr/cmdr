<script lang="ts">
    /**
     * A share list's account in the path bar: "as testuser" (or "as guest") after the
     * server's name, as a button that opens the server's account menu (Sign in as…, Browse as
     * guest where a guest listing worked, Forget saved password). With no listing to say who
     * it's signed in as yet, it offers "Sign in as…" itself, so the door is always there.
     *
     * ❗ One door of three to the same actions (the hub's row menu, the palette): the items are
     * `row-menu.ts::accountMenu`, a pick runs `share-list-sign-in.ts::runServerAccountAction`.
     * ❗ It reads no password: the account comes from what this session's listings said.
     *
     * Pane chrome: a plain button in the location bar's `data-pane-chrome` group, so the
     * chrome's arrow-key focus reaches it (`../pane/pane-chrome.ts`), ❌ no roving `tabindex`.
     * Its menu is a member of the chip's ONE `openMenu` (`VolumeBreadcrumb.svelte`), reported
     * through `onOpenChange`, so `isHeaderMenuOpen()` stays the single answer the panes
     * suppress keys on.
     *
     * The same menu is a share ROW's menu on this list (`openRowMenu`: right-click or ⌃⏎ on a
     * share): Open (mounts it, or opens a saved one's place), Copy path, a saved share's pin and
     * Forget share, then the server's account items
     * (`row-menu.ts::shareRowMenu`). Hosted here, ❌ not in FilePane (at its size cap), and
     * ❌ never the native file menu, whose Copy, Delete, and Reveal are all wrong for a share.
     */
    import { onDestroy } from 'svelte'
    import Menu from '#lib/ui/Menu.svelte'
    import { createMenu } from '#lib/ui/menu-controller.svelte.js'
    import { captureFocusOrigin, restoreFocus, type FocusOrigin } from '#lib/ui/focus-restore.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import {
        accountMenu,
        rowMenuSections,
        shareRowMenu,
        type ServerRowMenuEntry,
        type ShareRowAction,
    } from '../navigation/row-menu'
    import { runServerRowAction } from '../navigation/server-row-actions'
    import { forgetSavedShare } from './servers-hub-actions'
    import type { FileEntry } from '../types'
    import type { MenuAnchor } from '#lib/tauri-commands/index.js'
    import { getAppLogger } from '#lib/logging/logger.js'
    import { runServerAccountAction, serverAccountFacts } from './share-list-sign-in'
    import { shareListServerOf } from './share-list-label'
    import { signedInAsLabel } from './signed-in-as-label'

    interface Props {
        /** The share list's path (`smb://<server>`), which names the server. */
        path: string
        /** An account action listed the server: the pane re-lists, through its normal path. */
        onListed: () => void
        /** So the chip can keep one header menu open at a time (`VolumeBreadcrumb.noteAccountMenu`). */
        onOpenChange: (change: { open: boolean; close: () => void }) => void
        /** A share row's Open: the pane mounts the share, as Enter does. */
        onOpenShare: (entry: FileEntry) => void
    }

    const { path, onListed, onOpenChange, onOpenShare }: Props = $props()

    const log = getAppLogger('network')

    const server = $derived(shareListServerOf(path))
    const facts = $derived(server ? serverAccountFacts(server.host) : null)
    const label = $derived(
        facts?.listedAccount ? signedInAsLabel(facts.listedAccount) : tString('fileExplorer.network.share.signInAs'),
    )

    let button: HTMLButtonElement | undefined = $state()
    let focusBeforeOpen: FocusOrigin | null = null

    /** The share row whose menu is up, or `null` for the account's own. */
    let row = $state<FileEntry | null>(null)
    /** Where the last right-click landed, for a row menu the list opens with no anchor of its own. */
    let pointer: MenuAnchor | null = null

    // Read live, so a listing that lands under the open menu moves its items.
    const menu = createMenu<ServerRowMenuEntry>({
        getSections: () => {
            if (!server || !facts) return []
            return row
                ? rowMenuSections(row.path, shareRowMenu(facts, row.shareMeta?.saved ?? undefined), (entry) => entry)
                : rowMenuSections(path, accountMenu(facts), (entry) => entry)
        },
        onSelect: (item) => {
            const entry = item.data
            const picked = server
            const share = row
            if (entry?.type === 'share' && share) {
                void runShareVerb(entry.share, share)
                return
            }
            if (entry?.type !== 'account' || !picked) return
            void runServerAccountAction(entry.account, picked).then(({ listed }) => {
                if (listed) onListed()
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

    function toggle(): void {
        if (!button) return
        if (!menu.isOpen) focusBeforeOpen = captureFocusOrigin()
        row = null
        menu.toggleUnder(button)
    }

    /**
     * A share row's menu, at `anchor` (⌃⏎ passes the cursor row's) or where the right-click
     * landed. `..` has none: it's no share.
     */
    export function openRowMenu(entry: FileEntry, anchor: MenuAnchor | null | undefined): void {
        const at = anchor ?? pointer
        if (entry.name === '..' || !at) return
        focusBeforeOpen = captureFocusOrigin()
        row = entry
        menu.openAt(at)
    }

    /**
     * A share row's verb. A saved share's pin and Forget share act on its PLACE (the volume id
     * its row carries, `ShareMeta.saved`), through what the switcher's and the hub's menus run;
     * the row's dot follows with the re-list their `volumes-changed` brings.
     */
    async function runShareVerb(verb: ShareRowAction, share: FileEntry): Promise<void> {
        const saved = share.shareMeta?.saved
        switch (verb) {
            case 'open-share':
                onOpenShare(share)
                return
            case 'copy-path':
                await copyPath(share.path)
                return
            case 'pin':
            case 'unpin':
                if (saved) await runServerRowAction({ action: verb, volumeId: saved.volumeId, volumeName: share.name })
                return
            case 'forget-share':
                if (saved) await forgetSavedShare(saved.volumeId, share.name)
                return
        }
    }

    async function copyPath(text: string): Promise<void> {
        try {
            await navigator.clipboard.writeText(text)
        } catch (error) {
            log.warn("Couldn't copy the share's path: {error}", { error })
        }
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

{#if server}
    <button
        type="button"
        class="share-list-account borderless-control"
        class:is-open={menu.isOpen}
        bind:this={button}
        aria-haspopup="menu"
        aria-expanded={menu.isOpen}
        aria-label={tString('fileExplorer.network.share.accountMenuLabel', { account: label, hostName: server.label })}
        onclick={(e: MouseEvent) => {
            e.stopPropagation()
            toggle()
        }}
    >
        {label}
        <span class="chevron"></span>
    </button>
    <Menu {menu} ariaLabel={server.label} />
{/if}

<style>
    /* The breadcrumb's own trigger look (`VolumeBreadcrumb.svelte`'s `.breadcrumb-options-trigger`): muted until
       pointed at, so the account reads as a quiet aside to the server's name. */
    .share-list-account {
        font: inherit;
        margin: 0 0 0 var(--spacing-xxs);
        border: none;
        cursor: default;
        color: var(--color-text-tertiary);
        padding: var(--spacing-xxs) var(--spacing-xs);
        border-radius: var(--radius-sm);
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        white-space: nowrap;
        flex-shrink: 0;
        transition:
            background-color var(--transition-fast),
            color var(--transition-fast);
    }

    .share-list-account:hover,
    .share-list-account.is-open {
        color: var(--color-text-primary);
        background-color: var(--color-bg-tertiary);
    }

    .chevron {
        /* The chip's CSS triangle, colored by the button. */
        display: inline-block;
        width: 0;
        height: 0;
        border-left: 4px solid transparent;
        border-right: 4px solid transparent;
        border-top: 5px solid currentcolor;
        vertical-align: middle;
    }
</style>
