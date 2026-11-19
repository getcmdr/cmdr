<script lang="ts">
    /**
     * The viewer window's menu bar, drawn in the page where the OS doesn't own one
     * (`pageDrawsMenuBar()`: Windows and Linux; D5, task 63 of `docs/specs/windows-port-plan.md`).
     * The rows are the backend's answer for this viewer's own state (`get_viewer_menu_bar`, asked
     * again whenever one of the inputs moves; there's no event), and a pick goes back through
     * `pick_menu_bar_item` to this viewer's lanes, as a native click does. The viewer's own
     * Alt+letter commands keep their letters (`keyScope`).
     */
    import MenuBar from '#lib/ui/MenuBar.svelte'
    import type { BarMenuView, ViewerBarInputs } from '#lib/ipc/bindings.js'
    import { getViewerMenuBar, pickMenuBarItem } from '#lib/tauri-commands/index.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { getAppLogger } from '#lib/logging/logger.js'

    interface Props {
        /** The viewer's state the bar's checks and greying follow. */
        inputs: ViewerBarInputs
    }

    const { inputs }: Props = $props()

    const log = getAppLogger('inPageMenus')

    let menus = $state<BarMenuView[]>([])
    /** Each ask's number: an answer to an older ask than the newest is dropped. */
    let asked = 0

    $effect(() => {
        const current = { ...inputs }
        const ask = ++asked
        getViewerMenuBar(current)
            .then((next) => {
                if (ask === asked) menus = next
            })
            .catch((error: unknown) => {
                log.warn("Couldn't get the viewer's menu bar: {error}", { error })
            })
    })

    function pick(menuId: string): void {
        pickMenuBarItem(menuId).catch((error: unknown) => {
            log.warn("Couldn't run a menu bar pick: {error}", { error })
        })
    }
</script>

<MenuBar {menus} ariaLabel={tString('ui.menuBar.label')} keyScope="Viewer window" onPick={pick} />
