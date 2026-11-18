<script lang="ts">
    /**
     * An in-page right-click menu (D5: Windows draws its context menus in the page), on the same
     * surfaces as the menu bar's flyouts. Mount one per window and open it through its controller:
     * `const menu = createContextMenu({ onPick })` (`command-menu/context-menu.svelte.ts`), then `menu.open({ rows, at, fromKeyboard })`
     * with the rows `open_context_menu_in_page` answered. Nothing shows while it's closed.
     * Behavior: `command-menu/context-menu.svelte.ts`. Mechanics: `DETAILS.md` § Command menus.
     */
    import { untrack } from 'svelte'
    import { isAnySoftDialogOpen } from './open-dialogs.svelte'
    import MenuSurfaces from './command-menu/MenuSurfaces.svelte'
    import type { ContextMenuController } from './command-menu/context-menu.svelte'

    interface Props {
        menu: ContextMenuController
        /** Extra classes on its surfaces: a look of its own (the demo's macOS menus). */
        class?: string
    }

    const { menu, class: lookClass = '' }: Props = $props()

    // A dialog opening closes the menu, as a native one closes under a modal.
    $effect(() => {
        if (isAnySoftDialogOpen()) untrack(() => {
            menu.close()
        })
    })

    $effect(() => () => {
        menu.destroy()
    })
</script>

<MenuSurfaces stack={menu.stack} flavor={menu.flavor} cuesShown={menu.cuesShown} owner="context" class={lookClass} />
