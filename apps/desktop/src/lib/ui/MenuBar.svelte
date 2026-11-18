<script lang="ts">
    /**
     * The in-window menu bar (D5's `windowMenuBar`), drawn from the backend's answer
     * (`get_menu_bar`, `cmdr-menu`'s `page` rows), with its flyouts: one implementation for
     * Windows, KDE, other Linux desktops, and the browser demo's macOS bar. It draws and walks the
     * rows; a pick hands back the row's menu id, which the caller sends the way a native click
     * goes (`pick_menu_bar_item`). Behavior: `command-menu/menu-bar.svelte.ts`; the look:
     * `app-menu.css`'s tokens. Mechanics: `DETAILS.md` § Command menus.
     */
    import { untrack } from 'svelte'
    import { dependOn } from '#lib/utils/reactivity.js'
    import type { BarMenuView } from '#lib/ipc/bindings.js'
    import type { CommandScope } from '#lib/commands/types.js'
    import { convention } from '#lib/platform/conventions.js'
    import { reservedAccessKeys } from './access-keys'
    import { isAnySoftDialogOpen } from './open-dialogs.svelte'
    import MenuSurfaces from './command-menu/MenuSurfaces.svelte'
    import { createMenuBar, titleId } from './command-menu/menu-bar.svelte'
    import { labelParts, rowLabelText, type MenuFlavor } from './command-menu/rows'

    interface Props {
        /** The bar's menus, as the backend answers them (`get_menu_bar`), kept live by the caller. */
        menus: readonly BarMenuView[]
        /** A row was picked, by menu id. Focus is already back where it was. */
        onPick: (menuId: string) => void
        ariaLabel: string
        /** How it behaves. Defaults to this platform's: `mac` where the OS owns the bar, else `desktop`. */
        flavor?: MenuFlavor
        /** The window's command scope: its own Alt+letter commands keep their letters (the viewer's `Alt+R`). */
        keyScope?: CommandScope
        /** True while a modal surface is up, beyond the dialogs this already follows (the palette). */
        inert?: boolean
        /**
         * Whether Alt, F10, and Alt+letter (⌃F2 on the Mac) reach this bar from anywhere in the
         * window. Leave it on for a window's one bar; a second bar on the same page turns it off.
         */
        takesWindowKeys?: boolean
        /** Extra classes on the bar and its menus: a look of its own (the demo's macOS bar). */
        class?: string
    }

    const {
        menus,
        onPick,
        ariaLabel,
        flavor = convention('menuModel') === 'systemMenuBar' ? 'mac' : 'desktop',
        keyScope,
        inert = false,
        takesWindowKeys = true,
        class: lookClass = '',
    }: Props = $props()

    const uid = $props.id()
    const barId = `menu-bar-${uid}`
    let barElement: HTMLElement | undefined = $state()
    const titleElements: (HTMLElement | undefined)[] = $state([])

    const isInert = $derived(inert || isAnySoftDialogOpen())

    const bar = createMenuBar({
        getMenus: () => menus,
        onPick: (menuId) => {
            onPick(menuId)
        },
        flavor: () => flavor,
        accessKeys: () => convention('accessKeys') !== 'none',
        reservedLetters: () => reservedAccessKeys(keyScope),
        isInert: () => isInert,
        takesWindowKeys: () => takesWindowKeys,
        barElement: () => barElement,
        titleElement: (index) => titleElements[index],
    })

    // Once per mount: untracked, or a state read inside would tear the listeners down mid-use.
    $effect(() => untrack(() => bar.attach()))

    // A modal surface coming up takes the bar's keyboard and closes its menu, as Win32 disables an
    // owner window's bar under a modal dialog.
    $effect(() => {
        if (isInert) untrack(() => {
            bar.close()
        })
    })

    // The rows moved under an open menu (a live check, a label): redrawn in place.
    $effect(() => {
        dependOn(menus)
        untrack(() => {
            bar.refresh()
        })
    })

    // The keyboard's title holds focus while no menu is open (an open menu holds it itself).
    $effect(() => {
        const index = bar.focusIndex
        if (!bar.active || bar.openIndex !== null) return
        const title = titleElements[index]
        if (title && document.activeElement !== title) title.focus({ preventScroll: true })
    })

    function tabIndexOf(index: number): 0 | -1 {
        // A Mac bar is a tab stop at its first title, as the demo's always was; Windows' isn't one
        // until Alt or F10 hands it the keyboard.
        if (flavor === 'mac') return index === bar.focusIndex ? 0 : -1
        return bar.active && index === bar.focusIndex ? 0 : -1
    }
</script>

<div
    bind:this={barElement}
    id={barId}
    class="menu-bar {lookClass}"
    class:mac={flavor === 'mac'}
    class:cues-shown={bar.cuesShown}
    role="menubar"
    aria-label={ariaLabel}
    inert={isInert}
>
    {#each menus as menu, index (menu.id || index)}
        {@const parts = labelParts(rowLabelText(menu.title), bar.titleLetters[index] ?? null)}
        {@const letter = bar.titleLetters[index]}
        <div
            bind:this={titleElements[index]}
            id={titleId(barId, index)}
            class="menu-bar-title"
            class:is-open={bar.openIndex === index}
            role="menuitem"
            tabindex={tabIndexOf(index)}
            aria-haspopup="menu"
            aria-expanded={bar.openIndex === index}
            aria-controls={bar.openIndex === index ? bar.stack.surfaces.at(0)?.id : undefined}
            aria-keyshortcuts={letter ? `Alt+${letter.toUpperCase()}` : undefined}
            onpointerdown={(event: PointerEvent) => {
                bar.pointerDownTitle(index, event)
            }}
            onpointerenter={() => {
                bar.pointerEnterTitle(index)
            }}
        >
            {parts.before}{#if parts.key}<span class="menu-bar-access-key">{parts.key}</span>{/if}{parts.after}{#if parts.suffix}<span
                    aria-hidden="true"
                    > (<span class="menu-bar-access-key">{parts.suffix}</span>)</span
                >{/if}
        </div>
    {/each}
</div>

<MenuSurfaces stack={bar.stack} {flavor} cuesShown={bar.cuesShown} owner="bar" class={lookClass} />

<style>
    /* The bar's tokens (`app-menu.css` § Menu bar): WinUI's `MenuBar` on Windows, the house look
       elsewhere. No motion. */
    .menu-bar {
        display: flex;
        flex: none;
        align-items: center;
        box-sizing: border-box;
        height: var(--spacing-menu-bar-height);
        padding: 0 var(--spacing-menu-bar-padding-inline);
        background: var(--color-menu-bar-bg);
        color: var(--color-menu-bar-item-fg);
        font-family: var(--font-menu-family);
        font-size: var(--font-menu-bar-size);
        user-select: none;
        -webkit-user-select: none;
        cursor: default;
    }

    /* Contrast themes: on the Windows look the tokens already speak system colors, so the browser
       mustn't force them again (`app-menu.css` § Windows contrast themes). The guard is the Safari
       15.4 floor's (`scripts/vite-webkit-css-floor.ts`). */
    @supports (forced-color-adjust: none) {
        .menu-bar {
            forced-color-adjust: var(--color-menu-forced-adjust);
        }
    }

    .menu-bar-title {
        display: flex;
        align-items: center;
        box-sizing: border-box;
        height: var(--spacing-menu-bar-item-height);
        margin: 0 var(--spacing-menu-bar-item-margin-inline);
        padding: var(--spacing-menu-bar-item-padding-block) var(--spacing-menu-bar-item-padding-inline);
        border: var(--spacing-menu-bar-item-border) solid var(--color-menu-bar-item-border);
        border-radius: var(--radius-menu-bar-item);
        white-space: nowrap;
        outline: none;
    }

    .menu-bar-title:hover {
        background: var(--color-menu-bar-item-bg-hover);
        border-color: var(--color-menu-bar-item-border-hover);
    }

    /* The keyboard's title: its own fill (none on Windows) and the system focus rings. */
    .menu-bar-title:focus-visible {
        background: var(--color-menu-bar-item-bg-focus);
        box-shadow:
            inset 0 0 0 var(--spacing-menu-focus-outer) var(--color-menu-focus-outer),
            inset 0 0 0 calc(var(--spacing-menu-focus-outer) + var(--spacing-menu-focus-inner))
                var(--color-menu-focus-inner);
    }

    .menu-bar-title:active {
        background: var(--color-menu-bar-item-bg-pressed);
    }

    .menu-bar-title.is-open {
        background: var(--color-menu-bar-item-bg-open);
        border-color: var(--color-menu-bar-item-border-hover);
    }

    .cues-shown .menu-bar-access-key {
        text-decoration: underline;
        text-decoration-skip-ink: none;
    }
</style>
