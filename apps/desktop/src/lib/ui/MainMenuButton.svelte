<script lang="ts">
    /**
     * The main-menu button (D5): one button holding every menu of the bar as a submenu. GNOME's
     * "Main menu" (`primaryMenu`, which also ends it with GNOME's standard group and drops Quit),
     * and the stand-in for a bar the user hid on Windows and KDE (`command-menu/menu-layout.ts`).
     * It draws the rows the backend answered (`get_menu_bar`, kept live by the caller), on the
     * same surfaces as the bar and the context menus; a pick hands back the row's menu id, which
     * the caller sends through `pick_menu_bar_item`. Mechanics: `DETAILS.md` § Command menus.
     */
    import { untrack } from 'svelte'
    import type { BarMenuView, BarPickVia } from '#lib/ipc/bindings.js'
    import { convention } from '#lib/platform/conventions.js'
    import { tooltip } from '#lib/tooltip/tooltip.js'
    import { isTextInputFocused } from '#lib/utils/text-input-focus.js'
    import { dependOn } from '#lib/utils/reactivity.js'
    import Icon from './Icon.svelte'
    import ContextMenu from './ContextMenu.svelte'
    import { createContextMenu } from './command-menu/context-menu.svelte'
    import { primaryMenuRows } from './command-menu/menu-layout'
    import { isAnySoftDialogOpenBesides } from './open-dialogs.svelte'

    interface Props {
        /** The bar's menus, as the backend answers them (`get_menu_bar`), kept live by the caller. */
        menus: readonly BarMenuView[]
        /**
         * A row was picked, by menu id, and `via` a pointer click or a key (F10 then letters,
         * arrows then Enter). The menu is closed and focus is back where it was.
         */
        onPick: (menuId: string, via: BarPickVia) => void
        /** Its tooltip and accessible name ("Main menu"). */
        label: string
        /** F10 opens it from anywhere in the window: GNOME's HIG key, where no bar takes it. */
        opensOnF10?: boolean
        /** True while a modal surface is up, beyond the dialogs it follows itself (the palette). */
        inert?: boolean
    }

    const { menus, onPick, label, opensOnF10 = false, inert = false }: Props = $props()

    let button: HTMLButtonElement | undefined = $state()
    let expanded = $state(false)

    // As the bar does: inert under a dialog, but live over the inline rename, whose field keeps
    // focus while the menu is up (`focusHolder`), so its Edit rows reach the name.
    const isInert = $derived(inert || isAnySoftDialogOpenBesides('rename'))

    const rows = $derived(primaryMenuRows(menus, { standardGroup: convention('menuModel') === 'primaryMenu' }))

    const menu = createContextMenu({
        onPick: (menuId, via) => {
            onPick(menuId, via)
        },
        accessKeys: () => convention('accessKeys') !== 'none',
        onClose: () => {
            expanded = false
        },
    })

    function show(fromKeyboard: boolean): void {
        if (!button || isInert) return
        const rect = button.getBoundingClientRect()
        // A text field being edited keeps focus: the menu's cursor is its `aria-activedescendant`.
        const active = document.activeElement
        const focusHolder = isTextInputFocused() && active instanceof HTMLElement ? active : undefined
        // At the button's trailing bottom corner: the menu flips left, so it hangs under the button.
        menu.open({ rows, at: { x: rect.right, y: rect.bottom }, fromKeyboard, label, opener: button, focusHolder })
        expanded = true
    }

    function onPointerDown(event: PointerEvent): void {
        if (event.button !== 0) return
        // Focus stays where it was (a pane row, a text field) until a pick or Escape gives it back.
        event.preventDefault()
        if (menu.isOpen) menu.close()
        else show(false)
    }

    /**
     * A press is the menu's own: the strip it sits in (the tab row) would otherwise focus its pane
     * on the click, or open a tab on a double-click, taking focus from where the menu gives it back.
     */
    function stopHere(event: MouseEvent): void {
        event.stopPropagation()
    }

    function onButtonKeyDown(event: KeyboardEvent): void {
        if (event.key !== 'Enter' && event.key !== ' ' && event.key !== 'ArrowDown') return
        event.preventDefault()
        show(true)
    }

    /** F10 from anywhere in the window, where this button is the window's menu. */
    function onWindowKeyDown(event: KeyboardEvent): void {
        if (!opensOnF10 || isInert || event.isComposing || event.defaultPrevented) return
        if (event.key !== 'F10' || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
        event.preventDefault()
        event.stopPropagation()
        show(true)
    }

    $effect(() => {
        window.addEventListener('keydown', onWindowKeyDown, true)
        return () => {
            window.removeEventListener('keydown', onWindowKeyDown, true)
        }
    })

    // The rows moved under the open menu (a live check, a label): redrawn in place.
    $effect(() => {
        dependOn(rows)
        untrack(() => {
            if (menu.isOpen) menu.stack.refresh(rows)
        })
    })

    // A modal surface coming up closes it.
    $effect(() => {
        if (isInert) untrack(() => {
            menu.close()
        })
    })
</script>

<button
    bind:this={button}
    type="button"
    class="main-menu-button borderless-control"
    class:is-open={expanded}
    aria-label={label}
    aria-haspopup="menu"
    aria-expanded={expanded}
    inert={isInert}
    use:tooltip={{ text: label }}
    onpointerdown={onPointerDown}
    onkeydown={onButtonKeyDown}
    onclick={stopHere}
    ondblclick={stopHere}
>
    <Icon name="menu" size={16} aria-hidden="true" />
</button>

<ContextMenu {menu} />

<style>
    .main-menu-button {
        display: flex;
        flex: none;
        align-items: center;
        justify-content: center;
        align-self: center;
        padding: var(--spacing-xxs);
        border: none;
        border-radius: var(--radius-sm);
        background: none;
        color: var(--color-text-secondary);
        cursor: default;
    }

    .main-menu-button:hover,
    .main-menu-button.is-open {
        background-color: color-mix(in srgb, var(--color-bg-tertiary), transparent 40%);
        color: var(--color-text-primary);
    }
</style>
