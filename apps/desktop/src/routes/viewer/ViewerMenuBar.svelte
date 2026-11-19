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
    import { drawsBar, type MenuLayout } from '#lib/ui/command-menu/menu-layout.js'
    import { barRowForKey } from '#lib/ui/command-menu/bar-keys.js'
    import type { BarMenuView, BarPickVia, ViewerBarInputs } from '#lib/ipc/bindings.js'
    import { getViewerMenuBar, pickMenuBarItem } from '#lib/tauri-commands/index.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { getAppLogger } from '#lib/logging/logger.js'

    interface Props {
        /** The viewer's state the bar's checks and greying follow. */
        inputs: ViewerBarInputs
        /** Where its menus live (`#lib/ui/command-menu/menu-layout.ts`): the bar, the bar hidden until Alt, or none. */
        layout: MenuLayout
        /** The bar's rows as last answered, bound out for the main-menu button the toolbar draws. */
        menus?: BarMenuView[]
    }

    // eslint-disable-next-line prefer-const -- `$bindable()` requires `let` destructuring
    let { inputs, layout, menus = $bindable([]) }: Props = $props()

    const log = getAppLogger('inPageMenus')

    /** Each ask's number: an answer to an older ask than the newest is dropped. */
    let asked = 0

    /** Where focus was when the bar took the keyboard (`MenuBar`'s `onFocusOrigin`). */
    let focusOrigin = $state<Element | null>(null)

    /**
     * Whether `element` is the search box, the one field Cut and Paste act on.
     * Regression anchor (review of #1265): opening a menu moves focus into it, which blurs the box,
     * so its own focus flag greyed Cut and Paste in the very menu that offered them.
     */
    function isSearchBox(element: Element | null): boolean {
        return element instanceof HTMLInputElement && element.closest('.search-bar') !== null
    }

    $effect(() => {
        const current = { ...inputs, searchFocused: inputs.searchFocused || isSearchBox(focusOrigin) }
        const ask = ++asked
        getViewerMenuBar(current)
            .then((next) => {
                if (ask === asked) menus = next
            })
            .catch((error: unknown) => {
                log.warn("Couldn't get the viewer's menu bar: {error}", { error })
            })
    })

    function pick(menuId: string, via: BarPickVia): void {
        pickMenuBarItem(menuId, via).catch((error: unknown) => {
            log.warn("Couldn't run a menu bar pick: {error}", { error })
        })
    }

    /**
     * A key a row shows fires that row, as the native bar's accelerator did (Close's Ctrl+W): the
     * viewer's own keydown has no lane for them. Attached after the page's window handler (this
     * component renders after the page's `<svelte:window>`), so a key the page took stays its own.
     */
    function onKeyDown(event: KeyboardEvent): void {
        const id = barRowForKey(event, menus)
        if (id === null) return
        event.preventDefault()
        pick(id, 'key')
    }
</script>

<svelte:window onkeydown={onKeyDown} />

<!-- No rows, no bar: a host that draws its own (the browser demo) answers none. -->
{#if menus.length > 0 && drawsBar(layout)}
    <MenuBar
        {menus}
        revealOnly={layout === 'barOnAlt'}
        ariaLabel={tString('ui.menuBar.label')}
        keyScope="Viewer window"
        onPick={(menuId: string, via: BarPickVia) => {
            pick(menuId, via)
        }}
        onFocusOrigin={(origin: Element | null) => {
            focusOrigin = origin
        }}
    />
{/if}
