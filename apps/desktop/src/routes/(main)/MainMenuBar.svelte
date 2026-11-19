<script lang="ts">
    /**
     * The main window's menu bar, drawn in the page where the OS doesn't own one
     * (`pageDrawsMenuBar()`: Windows and Linux; D5, task 63 of `docs/specs/windows-port-plan.md`),
     * from the rows `main-menus.svelte.ts` follows. Shown, or hidden until Alt or F10 on Windows
     * with "Show menu bar" off (`#lib/ui/command-menu/menu-layout.ts`). Behavior and look:
     * `#lib/ui/MenuBar.svelte`.
     */
    import MenuBar from '#lib/ui/MenuBar.svelte'
    import type { BarMenuView } from '#lib/ipc/bindings.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { pickMainMenuRow } from './main-menus.svelte'

    interface Props {
        /** The bar's menus, kept live by `main-menus.svelte.ts`. */
        menus: readonly BarMenuView[]
        /** A modal surface beyond the dialogs the bar follows itself is up: the palette, an explorer overlay. */
        inert: boolean
        /** The bar is hidden and shows only while Alt or F10 hands it the keyboard. */
        revealOnly: boolean
    }

    const { menus, inert, revealOnly }: Props = $props()
</script>

<!-- The window's banner landmark (it has no other off macOS), so the bar sits in one, as axe's
     `region` rule asks of all page content. -->
<!-- No rows, no bar: a host that draws its own (the browser demo) answers none. -->
{#if menus.length > 0}
    <header class="main-menu-bar">
        <MenuBar
            {menus}
            {inert}
            {revealOnly}
            ariaLabel={tString('ui.menuBar.label')}
            keyScope="Main window"
            onPick={pickMainMenuRow}
        />
    </header>
{/if}

<style>
    .main-menu-bar {
        flex: none;
    }
</style>
