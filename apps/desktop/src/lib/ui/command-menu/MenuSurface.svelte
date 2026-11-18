<script lang="ts">
    /**
     * One surface of a command menu: a flyout, a context menu, or a submenu. Draws `cmdr-menu`'s
     * rows with full menu ARIA, keeps itself inside the window (flipping or scrolling at an edge,
     * D5's accepted limit), and holds the roving focus while it's the surface the keyboard is in.
     * The look is `app-menu.css`'s tokens, ❌ never a value of its own.
     */
    import { tick } from 'svelte'
    import type { Row } from '#lib/ipc/bindings.js'
    import { placeAtPoint, placeBesideRow, placeOffAnchor } from '../anchored-placement'
    import type { MenuStack, StackSurface } from './menu-stack.svelte'
    import { isLive, labelParts, macShortcutGlyphs, rowLabelText, type MenuFlavor } from './rows'

    interface Props {
        stack: MenuStack
        surface: StackSurface
        depth: number
        flavor: MenuFlavor
        cuesShown: boolean
        owner: 'bar' | 'context'
        lookClass: string
        /** The surface this submenu hangs off, for its parent row's rect. */
        parentElement: HTMLElement | undefined
        /** The open submenu's id, which its row's `aria-controls` names. */
        childId: string | undefined
        element?: HTMLElement
    }

    let {
        stack,
        surface,
        depth,
        flavor,
        cuesShown,
        owner,
        lookClass,
        parentElement,
        childId,
        element = $bindable(),
    }: Props = $props()

    /** The margin a surface keeps off the window's edge. */
    const EDGE_MARGIN = 4

    /**
     * How a submenu sits beside its row: `gap` lifts it so its first row lines up with the parent
     * row, `overlap` pulls it over the parent's edge (negative pushes it off).
     */
    const SUBMENU_OFFSET: Record<MenuFlavor, { gap: number; overlap: number }> = {
        // The demo's measured macOS 27 menus: 2 px off the parent, first rows level.
        mac: { gap: 5, overlap: -2 },
        // WinUI: the presenter's 1 px border and 2 px padding plus the item's 2 px margin.
        desktop: { gap: 5, overlap: 0 },
    }

    let position = $state<{ left: number; top: number; maxHeight: number } | null>(null)

    const hasGlyphs = $derived(surface.rows.some((row) => row.kind === 'check'))

    /** The surface the keyboard is in, which holds DOM focus: the deepest with a cursor. */
    const isActive = $derived(stack.activeDepth === depth)

    const rowId = (index: number) => `${surface.id}-row-${String(index)}`

    async function place(): Promise<void> {
        await tick()
        const el = element
        if (!el) return
        const size = { width: el.offsetWidth, height: el.scrollHeight }
        const viewport = { width: window.innerWidth, height: window.innerHeight }
        const anchor = surface.anchor
        if (depth > 0) {
            const row = parentElement?.querySelector(`[data-row-index="${String(surface.parentIndex)}"]`)
            if (!row) return
            const offset = SUBMENU_OFFSET[flavor]
            position = placeBesideRow({ row: row.getBoundingClientRect(), size, viewport, margin: EDGE_MARGIN, ...offset })
        } else if (anchor?.kind === 'point') {
            position = placeAtPoint({ point: anchor, size, viewport, margin: EDGE_MARGIN })
        } else if (anchor) {
            position = placeOffAnchor({ anchor: anchor.rect, size, viewport, gap: 0, margin: EDGE_MARGIN })
        }
    }

    // Placed once its rows are laid out, and again when they change (a live label can widen it).
    $effect(() => {
        void surface.rows
        if (element) void place()
    })

    // The roving focus: the cursor's row, or the surface itself while it has none (a menu a click
    // opened), so its keys and its name reach a screen reader.
    $effect(() => {
        const el = element
        const index = surface.highlighted
        if (!el || !position || !isActive) return
        const target = index >= 0 ? el.querySelector<HTMLElement>(`#${CSS.escape(rowId(index))}`) : el
        if (!target || document.activeElement === target) return
        target.focus({ preventScroll: true })
        if (index >= 0) target.scrollIntoView({ block: 'nearest' })
    })

    function roleOf(row: Row): string {
        if (row.kind !== 'check') return 'menuitem'
        return row.checkKind === 'radio' ? 'menuitemradio' : 'menuitemcheckbox'
    }

    function isOpenSubmenuRow(index: number): boolean {
        return stack.surfaces.at(depth + 1)?.parentIndex === index
    }
</script>

<!-- Keys live on the owner's window listener (`menu-bar.svelte.ts`, `context-menu.svelte.ts`), so
     they reach the menu wherever focus landed. -->
<div
    bind:this={element}
    id={surface.id}
    class="command-menu-surface {lookClass}"
    class:mac={flavor === 'mac'}
    class:cues-shown={cuesShown}
    role="menu"
    tabindex="-1"
    aria-label={surface.label}
    aria-labelledby={surface.labelledBy}
    data-command-menu={owner}
    data-depth={depth}
    data-has-glyphs={hasGlyphs ? '' : undefined}
    style:left="{position?.left ?? 0}px"
    style:top="{position?.top ?? 0}px"
    style:max-height={position ? `${String(position.maxHeight)}px` : undefined}
    style:visibility={position ? 'visible' : 'hidden'}
>
    {#each surface.rows as row, index (index)}
        {#if row.kind === 'separator'}
            <div class="command-menu-separator" role="separator"></div>
        {:else if row.kind === 'header'}
            <!-- The menu's name already (`aria-label`), so read once. -->
            <div class="command-menu-header" aria-hidden="true">{rowLabelText(row.label)}</div>
        {:else}
            {@const live = isLive(row)}
            {@const highlighted = surface.highlighted === index}
            {@const parts = labelParts(rowLabelText(row.label), surface.letters[index] ?? null)}
            {@const shortcut = row.kind === 'item' || row.kind === 'check' ? row.shortcut : null}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
                id={rowId(index)}
                class="command-menu-row"
                class:is-highlighted={highlighted}
                class:by-keyboard={highlighted && surface.highlightBy === 'keyboard'}
                class:is-open={isOpenSubmenuRow(index)}
                class:is-disabled={!live}
                role={roleOf(row)}
                tabindex={highlighted ? 0 : -1}
                aria-checked={row.kind === 'check' ? row.checked : undefined}
                aria-disabled={live ? undefined : 'true'}
                aria-haspopup={row.kind === 'submenu' ? 'menu' : undefined}
                aria-expanded={row.kind === 'submenu' ? isOpenSubmenuRow(index) : undefined}
                aria-controls={row.kind === 'submenu' && isOpenSubmenuRow(index) ? childId : undefined}
                aria-keyshortcuts={(row.kind === 'item' || row.kind === 'check' ? row.ariaKeyshortcuts : null) ?? undefined}
                data-row-index={index}
                data-menu-id={row.kind === 'item' || row.kind === 'check' || row.kind === 'submenu' ? row.id : undefined}
                onpointerenter={() => {
                    stack.hoverRow(depth, index)
                }}
                onpointerdown={(event: PointerEvent) => {
                    // Focus stays in the menu: a pick closes it and puts focus back itself.
                    event.preventDefault()
                }}
                onclick={() => {
                    stack.clickRow(depth, index)
                }}
            >
                <span class="command-menu-glyph" aria-hidden="true">
                    {#if row.kind === 'check' && row.checked}
                        <span class="command-menu-glyph-box">
                            {#if row.checkKind === 'radio' && flavor === 'desktop'}
                                <svg viewBox="0 0 12 12"><circle cx="6" cy="6" r="3" fill="currentColor" /></svg>
                            {:else}
                                <svg viewBox="0 0 12 12"
                                    ><path
                                        d="M2 6.5 4.8 9.3 10 3"
                                        fill="none"
                                        stroke="currentColor"
                                        stroke-width="1.8"
                                        stroke-linecap="round"
                                        stroke-linejoin="round"
                                    /></svg
                                >
                            {/if}
                        </span>
                    {/if}
                </span>
                <span class="command-menu-label"
                    >{parts.before}{#if parts.key}<span class="command-menu-access-key">{parts.key}</span
                        >{/if}{parts.after}{#if parts.suffix}<span aria-hidden="true"
                            > (<span class="command-menu-access-key">{parts.suffix}</span>)</span
                        >{/if}</span
                >
                {#if shortcut}
                    <span class="command-menu-shortcut" aria-hidden="true">
                        {#if flavor === 'mac'}
                            {#each macShortcutGlyphs(shortcut) as glyph, at (at)}<span class="command-menu-shortcut-glyph"
                                    >{glyph}</span
                                >{/each}
                        {:else}
                            {shortcut}
                        {/if}
                    </span>
                {/if}
                {#if row.kind === 'submenu'}
                    <span class="command-menu-chevron" aria-hidden="true">
                        <svg viewBox="0 0 7 12"
                            ><path
                                d="M1.5 1.5 6 6l-4.5 4.5"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="1.6"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                            /></svg
                        >
                    </span>
                {/if}
            </div>
        {/if}
    {/each}
</div>

<style>
    /* Every value is a token from `app-menu.css`, keyed on `data-look`: the house look by default,
       Windows 11's under `data-look='windows'`, and a caller's own through `lookClass`. No motion:
       WinUI switches states instantly, and so does this. */
    .command-menu-surface {
        position: fixed;
        z-index: calc(var(--z-modal-top) + 2);
        box-sizing: border-box;
        min-width: var(--spacing-menu-surface-min-width);
        overflow-y: auto;
        padding: var(--spacing-menu-surface-padding-block) 0;
        /* The acrylic's luminosity layer is the fill, its tint painted over it (`menu-look-tokens.md`). */
        background-color: var(--color-menu-surface-bg);
        background-image: linear-gradient(var(--color-menu-surface-tint), var(--color-menu-surface-tint));
        -webkit-backdrop-filter: var(--glass-menu-backdrop);
        backdrop-filter: var(--glass-menu-backdrop);
        border: var(--spacing-menu-surface-border) solid var(--color-menu-surface-border);
        border-radius: var(--radius-menu-surface);
        box-shadow: var(--shadow-menu-surface);
        color: var(--color-menu-row-fg);
        font-family: var(--font-menu-family);
        font-size: var(--font-menu-size);
        line-height: var(--font-menu-line-height);
        user-select: none;
        -webkit-user-select: none;
        cursor: default;
        outline: none;
        /* Contrast themes: the tokens already speak system colors, so the browser mustn't force
           them again (it would drop a highlighted row's Highlight fill back to Canvas). */
        forced-color-adjust: none;
    }

    .command-menu-row {
        display: flex;
        align-items: center;
        box-sizing: border-box;
        min-height: var(--spacing-menu-row-min-height);
        margin: var(--spacing-menu-row-margin-block) var(--spacing-menu-row-margin-inline);
        padding: var(--spacing-menu-row-padding-top) var(--spacing-menu-row-padding-end)
            var(--spacing-menu-row-padding-bottom) var(--spacing-menu-row-padding-start);
        border-radius: var(--radius-menu-row);
        white-space: nowrap;
        outline: none;
    }

    /* The check and radio column: reserved in a menu that holds one (and always in the house
       look, whose empty width equals the full one). */
    .command-menu-glyph {
        display: inline-flex;
        align-items: center;
        flex: none;
        width: var(--spacing-menu-glyph-column-empty);
        color: var(--color-menu-check-fg);
    }

    [data-has-glyphs] .command-menu-glyph {
        width: var(--spacing-menu-glyph-column);
    }

    .command-menu-glyph-box {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: var(--spacing-menu-glyph-size);
        height: var(--spacing-menu-glyph-size);
    }

    /* The glyph inside its box, as Segoe Fluent Icons and SF Symbols draw it: a little inset. */
    .command-menu-glyph-box svg {
        width: 80%;
        height: 80%;
    }

    .command-menu-label {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .cues-shown .command-menu-access-key {
        text-decoration: underline;
        text-decoration-skip-ink: none;
    }

    .command-menu-shortcut {
        display: inline-flex;
        flex: none;
        margin-inline-start: var(--spacing-menu-shortcut-gap);
        color: var(--color-menu-shortcut-fg);
        font-size: var(--font-menu-shortcut-size);
        font-variant-numeric: tabular-nums;
    }

    /* A Mac spaces a shortcut's glyphs, each in a cell of its own. */
    .mac .command-menu-shortcut {
        /* The measured macOS 27 glyph spacing. */
        gap: 1px;
    }

    .command-menu-shortcut-glyph {
        min-width: 0.9em;
        text-align: center;
    }

    .command-menu-chevron {
        display: inline-flex;
        flex: none;
        margin-inline-start: var(--spacing-menu-shortcut-gap);
        color: var(--color-menu-chevron-fg);
    }

    .command-menu-chevron svg {
        width: 7px;
        height: 11px;
    }

    .command-menu-header {
        display: flex;
        align-items: center;
        box-sizing: border-box;
        min-height: var(--spacing-menu-header-min-height);
        max-width: 320px;
        margin: var(--spacing-menu-row-margin-block) var(--spacing-menu-row-margin-inline);
        padding: var(--spacing-menu-row-padding-top) var(--spacing-menu-row-padding-end)
            var(--spacing-menu-row-padding-bottom)
            calc(var(--spacing-menu-row-padding-start) + var(--spacing-menu-glyph-column));
        overflow: hidden;
        color: var(--color-menu-header-fg);
        font-size: var(--font-menu-header-size);
        white-space: nowrap;
        text-overflow: ellipsis;
    }

    .command-menu-separator {
        height: 1px;
        margin: var(--spacing-menu-separator-margin-block) var(--spacing-menu-separator-inset);
        background: var(--color-menu-separator);
    }

    /* The pointer's row: the hover fill. */
    .command-menu-row.is-highlighted:not(.by-keyboard) {
        background: var(--color-menu-row-bg-hover);
        color: var(--color-menu-row-fg-hover);
    }

    .command-menu-row.is-highlighted:not(.by-keyboard) .command-menu-glyph {
        color: var(--color-menu-row-fg-hover);
    }

    .command-menu-row.is-highlighted:not(.by-keyboard) .command-menu-shortcut {
        color: var(--color-menu-shortcut-fg-hover);
    }

    .command-menu-row.is-highlighted:not(.by-keyboard) .command-menu-chevron {
        color: var(--color-menu-chevron-fg-hover);
    }

    /* The keyboard's row: its own fill (none on Windows, where the focus rings mark it). */
    .command-menu-row.by-keyboard {
        background: var(--color-menu-row-bg-focus);
        color: var(--color-menu-row-fg-focus);
    }

    .command-menu-row.by-keyboard .command-menu-shortcut {
        color: var(--color-menu-shortcut-fg-focus);
    }

    .command-menu-row.by-keyboard .command-menu-chevron {
        color: var(--color-menu-chevron-fg-focus);
    }

    /* Windows' system focus visual: a 2 px outer ring and a 1 px inner one, drawn only on the
       row that holds focus (zero-width in the house look). */
    .command-menu-row.by-keyboard:focus {
        box-shadow:
            inset 0 0 0 var(--spacing-menu-focus-outer) var(--color-menu-focus-outer),
            inset 0 0 0 calc(var(--spacing-menu-focus-outer) + var(--spacing-menu-focus-inner))
                var(--color-menu-focus-inner);
    }

    /* A submenu row while its submenu is open. */
    .command-menu-row.is-open {
        background: var(--color-menu-row-bg-open);
        color: var(--color-menu-row-fg-open);
    }

    .command-menu-row:not(.is-disabled):active {
        background: var(--color-menu-row-bg-pressed);
    }

    .command-menu-row:not(.is-disabled):active .command-menu-chevron {
        color: var(--color-menu-chevron-fg-pressed);
    }

    .command-menu-row.is-disabled,
    .command-menu-row.is-disabled .command-menu-glyph,
    .command-menu-row.is-disabled .command-menu-chevron {
        color: var(--color-menu-row-fg-disabled);
    }

    .command-menu-row.is-disabled .command-menu-shortcut {
        color: var(--color-menu-shortcut-fg-disabled);
    }
</style>
