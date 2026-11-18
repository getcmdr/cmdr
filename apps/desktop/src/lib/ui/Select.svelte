<script lang="ts" module>
    /**
     * One item in a `Select`. `value` is the stable identity (compared and emitted as a string);
     * `label` is the visible text; `description` renders as quieter inline text after the label
     * (used by `SettingSelect`'s option descriptions); `group` is an optional group/optgroup label
     * that, when present, buckets the item under an Ark `ItemGroup` with that heading; `iconUrl`
     * is an image shown before the label (an app-icon data URL, say), on the row and on the
     * trigger while that item is selected.
     */
    export interface SelectItem {
        value: string
        label: string
        description?: string
        group?: string
        /**
         * Image URL for a 16px icon rendered before the label. Decorative: the label carries the
         * meaning, so it renders with an empty `alt`. As soon as ONE item has an icon every row
         * reserves the space, so labels stay aligned down the list.
         */
        iconUrl?: string
        /**
         * Listed but can't be picked: pointer and keyboard skip it (Ark reads `disabled` off the item
         * by default), and it renders dimmed. Say why in `description`.
         */
        disabled?: boolean
        /**
         * BCP 47 tag of the language `label` is written in, when it isn't the UI's (a language
         * picker's endonyms). Lands as `lang` on the row and on the trigger's value text while
         * selected, so a screen reader switches voice for it.
         */
        lang?: string
    }
</script>

<script lang="ts">
    /**
     * Presentational, items-driven single-select built on Ark UI's `Select`. The house dropdown:
     * `SettingSelect` and the viewer / transfer / debug native-`<select>` replacements all render
     * through it so the macOS pop-up-button look stays in one place.
     *
     * macOS pop-up-button styling: a borderless trigger (value text + a round chevron stepper that
     * fills the whole trigger on hover/open), and a frosted-glass menu that opens *over* the trigger
     * with the currently-selected row landing on the trigger (see "macOS overlap positioning" below).
     * The checkmark marks the current value on the left; the accent highlight follows the keyboard /
     * pointer cursor.
     *
     * Stable class contract (don't rename without updating consumers + the a11y-contrast checker's
     * `scripts/check-a11y-contrast/dropdown_states.go`, which keys on literal selector strings):
     * `.select-trigger`, `.select-item`, `.select-content`, `.option-description`. `SettingSelect`'s
     * `handleCustomSubmit` focuses `.select-trigger` via `querySelector`. The `ariaLabel` lands on
     * the trigger.
     *
     * The open menu always portals out of the trigger's subtree (`portal-target.ts` picks where:
     * `document.body`, or the hosting dialog's overlay), so no ancestor clip, mask, or stacking
     * context can bury it. The overlap measurement finds the content through its `bind:ref`. See
     * `lib/ui/DETAILS.md` § Select.
     */
    import { Select, createListCollection, type SelectValueChangeDetails } from '@ark-ui/svelte/select'
    import { Portal } from '@ark-ui/svelte/portal'
    import { usePortalTarget } from '#lib/ui/portal-target.js'
    import Icon from '#lib/ui/Icon.svelte'
    import { computeOverlapShift } from '#lib/ui/select-positioning.js'
    import { tString } from '#lib/intl/messages.svelte.js'

    interface Props {
        items: SelectItem[]
        /** Selected item's `value`. Empty string means nothing selected (renders `placeholder`). */
        value: string
        onChange: (value: string) => void
        /** Fires on keyboard/pointer highlight. `SettingSelect` applies immediately on highlight. */
        onHighlightChange?: (highlightedValue: string | null) => void
        disabled?: boolean
        placeholder?: string
        ariaLabel: string
        /** Id of an element that explains the control, for example why it's disabled. */
        ariaDescribedBy?: string
        /** Extra class on the `.select-content` element (for example `custom-highlighted`). */
        contentClass?: string
    }

    const {
        items,
        value,
        onChange,
        onHighlightChange,
        disabled = false,
        placeholder,
        ariaLabel,
        ariaDescribedBy,
        contentClass = '',
    }: Props = $props()

    const resolvedPlaceholder = $derived(placeholder ?? tString('ui.select.placeholder'))

    const portalTarget = usePortalTarget()

    const collection = $derived(
        createListCollection({
            items,
            itemToString: (item: SelectItem) => item.label,
            itemToValue: (item: SelectItem) => item.value,
        }),
    )

    // Items render either flat or bucketed under their `group` label. Preserve first-seen group
    // order; items without a `group` bucket under '' (rendered as an unlabelled group).
    const groupedItems = $derived.by((): { label: string; items: SelectItem[] }[] | null => {
        const hasGroups = items.some((item) => item.group)
        if (!hasGroups) return null
        const groups: { label: string; items: SelectItem[] }[] = []
        for (const item of items) {
            const key = item.group ?? ''
            const existing = groups.find((g) => g.label === key)
            if (existing) existing.items.push(item)
            else groups.push({ label: key, items: [item] })
        }
        return groups
    })

    // One item with an icon makes the whole list reserve the slot, so labels line up whether or
    // not a given row has one. The trigger shows the selected item's icon for the same reason a
    // macOS pop-up button does: the button and the row it came from should read the same.
    const hasIcons = $derived(items.some((item) => item.iconUrl !== undefined))
    const selectedItem = $derived(items.find((item) => item.value === value))
    const selectedIconUrl = $derived(selectedItem?.iconUrl)

    function handleValueChange(details: SelectValueChangeDetails<SelectItem>): void {
        if (details.value.length > 0) onChange(details.value[0])
    }

    function handleHighlightChange(details: { highlightedValue: string | null }): void {
        onHighlightChange?.(details.highlightedValue)
    }

    // --- macOS overlap positioning ---------------------------------------------------------------
    // macOS opens a pop-up menu *over* the trigger, with the current value's row landing on the
    // button (its label aligned to the trigger's value text). Zag positions the *positioner* just
    // below the trigger (`bottom-start`, gutter 0); we then translate the *content* (a child of the
    // positioner, so this never fights zag's own transform) to slide the checked row onto the
    // trigger, clamped to the viewport so it stays on screen. Because the shift is a CSS transform on
    // the content it doesn't trigger a zag reposition, so there's no feedback loop.
    //
    // The reveal is driven by the open state, NOT zag's `onPositioned` (which this zag version never
    // fires). Content stays invisible (`positioned`) until the first measurement lands so it never
    // flashes at the default below-trigger spot; a `setTimeout` fallback guarantees it can never get
    // stuck invisible if rAF is throttled (unfocused window) or the rows aren't found.
    let rootEl: HTMLElement | null = $state(null)
    // The content's own ref, so the measurement finds it even when `portal` teleports it to body
    // (outside `rootEl`'s subtree). The trigger value stays inside `rootEl`.
    let contentEl: HTMLElement | null = $state(null)
    let isOpen = $state(false)
    let shiftX = $state(0)
    let shiftY = $state(0)
    let positioned = $state(false)

    const VIEWPORT_PAD = 8

    const positioning = {
        placement: 'bottom-start' as const,
        gutter: 0,
        flip: false,
        slide: true,
    }

    // Measure once the menu is open and zag has placed the positioner, then translate the content so
    // the checked row lands on the trigger. Returns false (so the caller retries on the next frame)
    // until the trigger value and checked row are both in the DOM.
    function alignToSelected(): boolean {
        if (!rootEl) return false
        const triggerValue = rootEl.querySelector('.select-value')
        const content = contentEl
        if (!triggerValue || !content) return false
        const checkedText =
            content.querySelector('.select-item[data-state="checked"] .select-item-text') ??
            content.querySelector('.select-item .select-item-text')
        if (!checkedText) return false

        const shift = computeOverlapShift({
            trigger: triggerValue.getBoundingClientRect(),
            item: checkedText.getBoundingClientRect(),
            content: content.getBoundingClientRect(),
            shiftX,
            shiftY,
            viewportWidth: window.innerWidth,
            viewportHeight: window.innerHeight,
            pad: VIEWPORT_PAD,
        })
        shiftX = shift.x
        shiftY = shift.y
        positioned = true
        return true
    }

    $effect(() => {
        if (!isOpen) return
        let cancelled = false
        let raf = 0
        // Retry across a few frames: the content mounts and zag places it asynchronously after open.
        // The measurement self-corrects, so an extra refine frame settles any residual gap.
        const attempt = (n: number): void => {
            if (cancelled) return
            const aligned = alignToSelected()
            if (n < 4) raf = requestAnimationFrame(() => { attempt(n + 1); })
            else if (!aligned) positioned = true
        }
        raf = requestAnimationFrame(() => { attempt(0); })
        const fallback = setTimeout(() => {
            if (!cancelled) positioned = true
        }, 60)
        return () => {
            cancelled = true
            cancelAnimationFrame(raf)
            clearTimeout(fallback)
        }
    })

    function handleOpenChange(details: { open: boolean }): void {
        isOpen = details.open
        if (details.open) {
            shiftX = 0
            shiftY = 0
            positioned = false
        }
    }

    // Applied as an inline style (not a CSS rule) because the overlap shift is per-instance dynamic.
    // The content is hidden until the first measurement lands, so it never flashes below the trigger.
    const contentStyle = $derived(
        `transform: translate(${String(shiftX)}px, ${String(shiftY)}px); opacity: ${positioned ? '1' : '0'}`,
    )
</script>

<div class="select-root" bind:this={rootEl}>
    <Select.Root
        class="select-inner"
        {collection}
        value={value ? [value] : []}
        onValueChange={handleValueChange}
        onHighlightChange={handleHighlightChange}
        onOpenChange={handleOpenChange}
        {positioning}
        {disabled}
    >
        <Select.Control>
            <Select.Trigger class="select-trigger" aria-label={ariaLabel} aria-describedby={ariaDescribedBy}>
                {#if selectedIconUrl}
                    <img class="select-item-icon" src={selectedIconUrl} alt="" width="16" height="16" />
                {/if}
                <Select.ValueText class="select-value" placeholder={resolvedPlaceholder} lang={selectedItem?.lang} />
                <span class="select-indicator"><Icon name="chevrons-up-down" size={14} aria-hidden="true" /></span>
            </Select.Trigger>
        </Select.Control>
        <!-- Ark's Portal forwards the Select context, and reads `container` only when it mounts, by
             which point a hosting dialog's overlay is bound. -->
        <Portal container={portalTarget()}>
            <Select.Positioner>
                <Select.Content
                    bind:ref={contentEl}
                    class={`select-content${contentClass ? ` ${contentClass}` : ''}`}
                    style={contentStyle}
                    onkeydown={(e: KeyboardEvent) => {
                        // Keep Escape scoped to the dropdown so a host dialog's capture-phase Escape
                        // doesn't also fire and close the whole dialog.
                        if (e.key === 'Escape') e.stopPropagation()
                    }}
                >
                    {#if groupedItems}
                        {#each groupedItems as group (group.label)}
                            <Select.ItemGroup>
                                {#if group.label}
                                    <Select.ItemGroupLabel class="select-group-label">
                                        {group.label}
                                    </Select.ItemGroupLabel>
                                {/if}
                                {#each group.items as item (item.value)}
                                    <Select.Item {item} class="select-item" lang={item.lang}>
                                        <Select.ItemIndicator class="item-indicator"
                                            ><Icon name="check" size={13} aria-hidden="true" /></Select.ItemIndicator
                                        >
                                        {#if hasIcons}
                                            <span class="select-item-icon">
                                                {#if item.iconUrl}<img
                                                        src={item.iconUrl}
                                                        alt=""
                                                        width="16"
                                                        height="16"
                                                    />{/if}
                                            </span>
                                        {/if}
                                        <Select.ItemText class="select-item-text">
                                            {item.label}
                                            {#if item.description}
                                                <span class="option-description"> — {item.description}</span>
                                            {/if}
                                        </Select.ItemText>
                                    </Select.Item>
                                {/each}
                            </Select.ItemGroup>
                        {/each}
                    {:else}
                        {#each items as item (item.value)}
                            <Select.Item {item} class="select-item" lang={item.lang}>
                                <Select.ItemIndicator class="item-indicator"
                                    ><Icon name="check" size={13} aria-hidden="true" /></Select.ItemIndicator
                                >
                                {#if hasIcons}
                                    <span class="select-item-icon">
                                        {#if item.iconUrl}<img src={item.iconUrl} alt="" width="16" height="16" />{/if}
                                    </span>
                                {/if}
                                <Select.ItemText class="select-item-text">
                                    {item.label}
                                    {#if item.description}
                                        <span class="option-description"> — {item.description}</span>
                                    {/if}
                                </Select.ItemText>
                            </Select.Item>
                        {/each}
                    {/if}
                </Select.Content>
            </Select.Positioner>
        </Portal>
        <Select.HiddenSelect />
    </Select.Root>
</div>

<style>
    .select-root {
        display: contents;
    }

    /* Ark `Select.Root`'s box. `.select-root` above is `display: contents` (no box
       of its own), so `Select.HiddenSelect` — hidden sr-only style with no
       `top`/`left` — would otherwise resolve against the window shell and make it
       secretly scrollable (see `Switch.svelte`'s `.switch-root` for the full why).
       Positioning this box keeps the hidden `<select>` local so the real scroller
       clips it. `relative` doesn't change the box's flow position. */
    :global(.select-inner) {
        position: relative;
    }

    /* macOS borderless pop-up button: value text + a round chevron stepper, hugging its content. */
    :global(.select-trigger) {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xs);
        min-height: var(--spacing-select-trigger-min-height);
        padding: var(--spacing-select-trigger-padding);
        border: none;
        border-radius: var(--radius-select-trigger);
        background: var(--color-select-trigger-bg);
        color: var(--color-select-trigger-fg);
        font-size: var(--font-select-trigger-size);
        box-shadow: var(--shadow-select-trigger);
        cursor: default;
    }

    /* The Safari 15.4 floor's guard (`scripts/vite-webkit-css-floor.ts`): every engine with contrast
       themes reads it. */
    @supports (forced-color-adjust: none) {
        :global(.select-trigger) {
            forced-color-adjust: var(--color-forced-adjust);
        }
    }

    /* On hover (and while open) the house trigger fills with the chevron stepper's color, so the
       control reads as one uniform pill — the macOS pop-up-button behavior. Windows' ComboBox is
       a bordered control-fill button that lightens instead. */
    :global(.select-trigger:hover:not([data-disabled])) {
        background: var(--color-select-trigger-bg-hover);
        box-shadow: var(--shadow-select-trigger-hover);
    }

    :global(.select-trigger[data-state='open']) {
        background: var(--color-select-trigger-bg-open);
        box-shadow: var(--shadow-select-trigger-hover);
    }

    :global(.select-value) {
        white-space: nowrap;
    }

    /* Every look dims a disabled trigger. On Windows that approximates WinUI's disabled ComboBox
       (ControlFillColorDisabled under TextFillColorDisabled), close enough for a rare state. */
    :global(.select-trigger[data-disabled]) {
        cursor: not-allowed;
        opacity: 0.5;
    }

    :global(.select-trigger:focus-visible) {
        outline: 2px solid var(--color-focus-ring);
        outline-offset: 1px;
        box-shadow: var(--shadow-focus-ring-select);
    }

    /* The macOS-style chevron stepper: a circle holding the up/down chevrons. Its fill matches the
       trigger's hover/open fill, so it blends into the pill in those states (one uniform color) and
       reads as a distinct circle only at rest. */
    :global(.select-indicator) {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        width: 18px;
        height: 18px;
        border-radius: var(--radius-full);
        background: var(--color-select-indicator-bg);
        color: var(--color-select-indicator-fg);
    }

    /* Frosted-glass menu. Shared tokens with tooltips / filter-chip popovers so every glass surface
       reads as one material; the blur is dropped under reduced transparency (token flips opaque). */
    :global(.select-content) {
        /* The dropdown rung goes on the CONTENT, and zag lifts it onto the positioner, the box that
           actually stacks. When zag places the menu it reads the content's computed `z-index` and
           writes it inline on the positioner as `--z-index`, which the positioner's inline
           `z-index: var(--z-index)` picks up. ❌ Never put the rung in a positioner class rule
           instead: zag's inline write outranks it, so in every window where the placement runs
           (every focused one) the positioner lands on `z-index: auto` and anything on a real rung
           paints over the menu. It stacks relative to wherever the menu portals
           (`portal-target.ts`): body level, or a hosting dialog's overlay, under `--z-modal`. */
        z-index: var(--z-dropdown);
        /* The flyout material (`app-menu.css`): the house glass, or Windows' acrylic. */
        background: var(--color-menu-surface-bg);
        -webkit-backdrop-filter: var(--glass-menu-backdrop);
        backdrop-filter: var(--glass-menu-backdrop);
        border: var(--spacing-menu-surface-border) solid var(--color-menu-surface-border);
        border-radius: var(--radius-menu-surface);
        box-shadow: var(--shadow-menu-surface);
        padding: var(--spacing-select-content-padding);
        max-height: 300px;
        overflow-y: auto;
        /* Consistent width regardless of content. */
        min-width: 180px;
        width: max-content;
        outline: none;
        /* The macOS overlap shift (transform) and the until-measured hide (opacity) are applied
           inline per instance; see `contentStyle`. */
    }

    /* The Safari 15.4 floor's guard (`scripts/vite-webkit-css-floor.ts`): every engine with contrast
       themes reads it. */
    @supports (forced-color-adjust: none) {
        :global(.select-content) {
            forced-color-adjust: var(--color-forced-adjust);
        }
    }

    :global(.select-content:focus),
    :global(.select-content:focus-visible) {
        outline: none;
    }

    :global(.select-group-label) {
        display: block;
        padding: var(--spacing-xs) var(--spacing-sm);
        font-size: var(--font-size-xs);
        font-weight: 600;
        color: var(--color-text-tertiary);
    }

    :global(.select-item) {
        display: flex;
        align-items: center;
        gap: var(--spacing-xs);
        margin: var(--spacing-select-item-margin);
        padding: var(--spacing-select-item-padding);
        border-radius: var(--radius-select-item);
        cursor: default;
        font-size: var(--font-select-item-size);
        outline: none;
    }

    /* An item's leading image (an app icon, say), and the same image on the trigger while that
       item is selected. Fixed box so an iconless row's empty span reserves identical space and
       the labels stay in one column. */
    :global(.select-item-icon) {
        flex-shrink: 0;
        width: 16px;
        height: 16px;
        object-fit: contain;
    }

    /* The label cell: takes the remaining width after the left checkmark. */
    :global(.select-item-text) {
        flex: 1;
        min-width: 0;
    }

    /* No hover visual indication on its own: only the highlighted (cursor) state paints. */
    :global(.select-item:hover) {
        background-color: transparent;
    }

    /* The current value's row: nothing extra in the house look (its checkmark marks it); on
       Windows a subtle fill and WinUI's accent pill on the leading edge, as a background layer. */
    :global(.select-item[data-state='checked']) {
        background-color: var(--color-select-item-bg-checked);
        background-image: var(--color-select-item-checked-mark);
        background-position: var(--spacing-select-item-checked-mark-position);
        background-size: var(--spacing-select-item-checked-mark-size);
        background-repeat: var(--spacing-select-item-checked-mark-repeat);
    }

    /* Highlighted item (keyboard / pointer cursor): accent fill, like macOS. */
    :global(.select-item[data-highlighted]) {
        background-color: var(--color-select-item-bg-highlighted);
        color: var(--color-select-item-fg-highlighted);
    }

    :global(.select-item[data-highlighted]:hover) {
        background-color: var(--color-select-item-bg-highlighted-hover);
    }

    /* A disabled row stays readable but dimmed, and the cursor never lands on it. */
    :global(.select-item[data-disabled]) {
        color: var(--color-text-tertiary);
    }

    :global(.select-item:focus),
    :global(.select-item:focus-visible) {
        outline: none;
    }

    /* Checkmark marks the current value, on the left, macOS-style. Always reserve its space so
       labels stay aligned whether or not a row is checked. */
    :global(.item-indicator) {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        min-width: 1em;
        color: var(--color-select-check);
        visibility: hidden;
    }

    :global(.select-item[data-state='checked'] .item-indicator) {
        visibility: visible;
    }

    /* On the highlighted row the checkmark sits on the accent fill, so it flips to the accent fg. */
    :global(.select-item[data-highlighted] .item-indicator) {
        color: var(--color-select-check-highlighted);
    }

    /* When a content-level class (`custom-highlighted`) is set, hide the checkmark on the still-checked
       standard row so the highlighted "Custom…" row is the only one reading as selected. */
    :global(.custom-highlighted .select-item[data-state='checked']:not([data-highlighted]) .item-indicator) {
        visibility: hidden;
    }

    .option-description {
        color: var(--color-text-tertiary);
        font-size: var(--font-size-sm);
    }

    /* When the parent option is highlighted the bg flips to `--color-accent`. The description's
       resting `--color-text-tertiary` (#666 / #a0a0a0) drops to ~1–2.4:1 contrast on every system
       accent. Switch it to `--color-accent-fg` (auto-picked black/white via `readableFgOn`) so it
       matches the label's color and stays readable. The secondary visual weight is lost in this
       state, but on a saturated bg there's no opacity that stays both secondary and AA-compliant for
       Apple Purple (the worst dark-fg case). The contrast checker's `dropdown_states.go` matrix
       validates this against every accent variant. */
    :global(.select-item[data-highlighted]) .option-description {
        color: var(--color-select-item-description-highlighted);
    }
</style>
