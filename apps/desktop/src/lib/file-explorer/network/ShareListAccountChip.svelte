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
     */
    import { onDestroy } from 'svelte'
    import Menu from '#lib/ui/Menu.svelte'
    import { createMenu } from '#lib/ui/menu-controller.svelte.js'
    import { captureFocusOrigin, restoreFocus, type FocusOrigin } from '#lib/ui/focus-restore.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { accountMenu, rowMenuSections, type ServerRowMenuEntry } from '../navigation/row-menu'
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
    }

    const { path, onListed, onOpenChange }: Props = $props()

    const server = $derived(shareListServerOf(path))
    const facts = $derived(server ? serverAccountFacts(server.host) : null)
    const label = $derived(
        facts?.listedAccount ? signedInAsLabel(facts.listedAccount) : tString('fileExplorer.network.share.signInAs'),
    )

    let button: HTMLButtonElement | undefined = $state()
    let focusBeforeOpen: FocusOrigin | null = null

    // Read live, so a listing that lands under the open menu moves its items.
    const menu = createMenu<ServerRowMenuEntry>({
        getSections: () => (server && facts ? rowMenuSections(path, accountMenu(facts), (entry) => entry) : []),
        onSelect: (item) => {
            const entry = item.data
            const picked = server
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
        menu.toggleUnder(button)
    }

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
