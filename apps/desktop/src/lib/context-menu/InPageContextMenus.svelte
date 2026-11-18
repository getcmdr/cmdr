<script lang="ts">
    /**
     * The window's in-page right-click menu, where `convention('contextMenus')` is `inPage`
     * (Windows): the root layout mounts it there, in every window. It draws the `show_*`
     * wrappers' menus (`popContextMenu`) and the text fields' edit menu. `DETAILS.md`.
     */
    import ContextMenu from '#lib/ui/ContextMenu.svelte'
    import { registerContextMenuDrawer } from '#lib/tauri-commands/index.js'
    import { createContextMenuHost } from './context-menu-host.svelte'
    import { setWindowContextMenuHost } from './window-context-menu'

    const host = createContextMenuHost()

    $effect(() => {
        const stopFollowing = host.install()
        const unregisterHost = setWindowContextMenuHost(host)
        const unregisterDrawer = registerContextMenuDrawer((answer) => host.draw(answer))
        return () => {
            unregisterDrawer()
            unregisterHost()
            stopFollowing()
        }
    })
</script>

<ContextMenu menu={host.menu} />
