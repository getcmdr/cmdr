<script lang="ts">
    /**
     * The main window's menu bar, drawn in the page where the OS doesn't own one
     * (`pageDrawsMenuBar()`: Windows and Linux; D5, task 63 of `docs/specs/windows-port-plan.md`).
     * The rows are the backend's (`get_menu_bar`, followed live by `followMenuBar`), and a pick goes
     * back through `pick_menu_bar_item`, the handler a native click reaches. Behavior and look:
     * `#lib/ui/MenuBar.svelte`.
     */
    import { onDestroy, onMount } from 'svelte'
    import MenuBar from '#lib/ui/MenuBar.svelte'
    import type { BarMenuView } from '#lib/ipc/bindings.js'
    import { followMenuBar, pickMenuBarItem } from '#lib/tauri-commands/index.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { getAppLogger } from '#lib/logging/logger.js'

    interface Props {
        /** A modal surface beyond the dialogs the bar follows itself is up: the palette, an explorer overlay. */
        inert: boolean
    }

    const { inert }: Props = $props()

    const log = getAppLogger('inPageMenus')

    let menus = $state<BarMenuView[]>([])
    let stop: (() => void) | undefined
    let destroyed = false

    onMount(() => {
        void followMenuBar((next) => {
            menus = next
        }).then((unlisten) => {
            if (destroyed) unlisten()
            else stop = unlisten
        })
    })

    onDestroy(() => {
        destroyed = true
        stop?.()
    })

    function pick(menuId: string): void {
        pickMenuBarItem(menuId).catch((error: unknown) => {
            log.warn("Couldn't run a menu bar pick: {error}", { error })
        })
    }
</script>

<MenuBar {menus} {inert} ariaLabel={tString('ui.menuBar.label')} keyScope="Main window" onPick={pick} />
