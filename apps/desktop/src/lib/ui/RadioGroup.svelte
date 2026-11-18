<script lang="ts" module>
    /**
     * One option in a `RadioGroup`. `value` is the stable identity (compared and emitted as a
     * string); `label` is the visible text; `description` renders as quieter text below the label
     * (used by settings' option descriptions); `disabled` greys out and blocks the single option.
     */
    export interface RadioItem {
        value: string
        label: string
        description?: string
        disabled?: boolean
    }
</script>

<script lang="ts">
    /**
     * Presentational, items-driven single-select radio group ("options") built on Ark UI's
     * `RadioGroup`. The house radio group: `SettingRadioGroup` and any raw `<input type="radio">`
     * group converge here so the styled control lives in one place. Ark owns keyboard a11y and ARIA
     * (`role="radiogroup"` with `role="radio"` items); we style the control dot and layout.
     */
    import { RadioGroup, type RadioGroupValueChangeDetails } from '@ark-ui/svelte/radio-group'
    import type { Snippet } from 'svelte'

    interface Props {
        /** Selected item's `value`. Empty string means nothing selected. Bindable. */
        value?: string
        items: RadioItem[]
        onValueChange?: (value: string) => void
        /** Group-level disable; short-circuits every option. */
        disabled?: boolean
        /** `vertical` stacks the options; `horizontal` lays them in a wrapping row. */
        orientation?: 'vertical' | 'horizontal'
        /**
         * Lays the options out in this many equal full-width columns, filling row by row
         * (5 options over 3 columns read as 3 + 2). Overrides `orientation`. Use it when a
         * wrapping row would break at an arbitrary place and leave the options looking
         * scattered; the grid gives them a shared left edge per column.
         */
        columns?: number
        /** Accessible name for the group root. */
        ariaLabel?: string
        /** Id of an element that explains the group, for example why it's disabled. */
        ariaDescribedBy?: string
        /**
         * Rendered after the items, receiving the current `value`. Preserves the "custom content when
         * a specific option is selected" feature; the caller decides visibility.
         */
        footer?: Snippet<[string]>
        /**
         * Rendered on the same line as each option, receiving that option's `value`; the caller
         * returns content for the one option it belongs to and nothing for the rest. It renders
         * BESIDE the option, never inside it: a focusable control nested in a `role="radio"`
         * element trips axe's nested-interactive rule.
         */
        itemTrailing?: Snippet<[string]>
        /**
         * Rendered INSIDE the option's label, right after the label text, receiving that option's
         * `value`. Use it for a badge or a run of helper text that should read as part of the
         * option: everything in here is clickable (the label is what selects the radio) and joins
         * the control's accessible name, which `itemTrailing` does neither of.
         *
         * ❌ Nothing focusable — that's what `itemTrailing` is for. A `string` description goes in
         * the item's `description` field instead; this is for markup.
         */
        itemInline?: Snippet<[string]>
        /**
         * Rendered under one option, receiving that option's `value`. For a note or panel that
         * belongs to a single choice, where the group-level `footer` would strand it at the bottom
         * of the list under whichever option happens to be last.
         */
        itemFooter?: Snippet<[string]>
    }

    /* eslint-disable prefer-const -- $bindable() requires `let` destructuring */
    let {
        value = $bindable(''),
        items,
        onValueChange,
        disabled = false,
        orientation = 'vertical',
        columns,
        ariaLabel,
        ariaDescribedBy,
        footer,
        itemTrailing,
        itemInline,
        itemFooter,
    }: Props = $props()
    /* eslint-enable prefer-const */

    function handleValueChange(details: RadioGroupValueChangeDetails): void {
        if (details.value) {
            value = details.value
            onValueChange?.(details.value)
        }
    }
</script>

<RadioGroup.Root
    {value}
    onValueChange={handleValueChange}
    {disabled}
    aria-label={ariaLabel}
    aria-describedby={ariaDescribedBy}
>
    <div
        class="radio-group"
        class:horizontal={orientation === 'horizontal' && columns === undefined}
        class:grid={columns !== undefined}
        style={columns === undefined ? undefined : `grid-template-columns: repeat(${String(columns)}, minmax(0, 1fr))`}
    >
        {#each items as item (item.value)}
            <!-- The cell is `display: contents` unless someone asked for a per-option footer,
                 so a group without one lays out exactly as it did before the wrapper existed:
                 the row itself stays the flex / grid item. -->
            <div class="radio-cell" class:has-footer={itemFooter !== undefined}>
                <div class="radio-row">
                    <!-- The described case is a DATA ATTRIBUTE, not a second class: a
                         computed `class` string hides the name from the unused-CSS
                         scanner, which then reads every `.radio-*` rule as dead. -->
                    <RadioGroup.Item
                        value={item.value}
                        class="radio-item"
                        data-described={item.description ? 'true' : undefined}
                        disabled={disabled || item.disabled}
                    >
                        <RadioGroup.ItemControl class="radio-control" />
                        <RadioGroup.ItemText class="radio-text">
                            {#if itemInline}
                                <!-- `.radio-text` is a COLUMN (label over description), so inline
                                     content needs its own row or it would stack under the label. -->
                                <span class="radio-label-line">
                                    <span class="radio-label">{item.label}</span>
                                    {@render itemInline(item.value)}
                                </span>
                            {:else}
                                <span class="radio-label">{item.label}</span>
                            {/if}
                            {#if item.description}
                                <span class="radio-description">{item.description}</span>
                            {/if}
                        </RadioGroup.ItemText>
                        <RadioGroup.ItemHiddenInput />
                    </RadioGroup.Item>
                    {#if itemTrailing}
                        {@render itemTrailing(item.value)}
                    {/if}
                </div>
                {#if itemFooter}
                    {@render itemFooter(item.value)}
                {/if}
            </div>
        {/each}
        {#if footer}
            {@render footer(value)}
        {/if}
    </div>
</RadioGroup.Root>

<style>
    .radio-group {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
    }

    /* Holds an option and its optional `itemTrailing` control on one line. With no trailing
       control the option is the row's only element child, so it stretches the way it did
       before the wrapper existed and the whole row width stays clickable. */
    .radio-row {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .radio-row > :global(.radio-item:only-child) {
        flex: 1 1 auto;
    }

    .radio-group.horizontal {
        flex-direction: row;
        flex-wrap: wrap;
        gap: var(--spacing-md);
    }

    /* Equal columns filling the full width, so a leftover row (5 options over 3
       columns) still lines its options up under the ones above. The track list is
       inline (the count is a prop): `repeat(n, minmax(0, 1fr))`, `minmax` rather
       than a bare `1fr` so a long label wraps inside its column instead of widening
       the whole grid. */
    .radio-group.grid {
        display: grid;
        gap: var(--spacing-xs) var(--spacing-md);
    }

    /* A plain option centers its dot on its label: with `flex-start` a single-line
       label sits visibly high against the 16px control, which reads as sloppy across
       a row of options. Only a described option top-aligns (`[data-described]`),
       because there the dot belongs beside the LABEL line, not the block's middle. */
    :global(.radio-item) {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        padding: var(--spacing-xs) 0;
        cursor: default;
        /* Contain each item's Ark `ItemHiddenInput` so it resolves against its
           own row, not the window shell. See `Switch.svelte`'s `.switch-root`
           for the full why (the shell-scroll-under-traffic-lights bug). */
        position: relative;
    }

    :global(.radio-item[data-described]) {
        align-items: flex-start;
    }

    /* Disabled re-points the state tokens at their disabled twins (the house twins equal
       what it painted before; it dims with opacity instead, below). */
    :global(.radio-item[data-disabled]) {
        --color-radio-bg: var(--color-radio-bg-disabled);
        --color-radio-bg-hover: var(--color-radio-bg-disabled);
        --color-radio-border: var(--color-radio-border-disabled);
        --color-radio-border-hover: var(--color-radio-border-disabled);
        --color-radio-checked-border: var(--color-radio-checked-border-disabled);
        --color-radio-checked-border-hover: var(--color-radio-checked-border-disabled-hover);
        --color-radio-checked-bg: var(--color-radio-checked-bg-disabled);
        --color-radio-checked-bg-hover: var(--color-radio-checked-bg-disabled-hover);
        --shadow-radio-checked: var(--shadow-radio-checked-disabled);
        --shadow-radio-checked-hover: var(--shadow-radio-checked-disabled);
        --color-radio-label: var(--color-radio-label-disabled);

        cursor: not-allowed;
    }

    /* The dim is the house look's disabled state; Windows paints WinUI's disabled colors
       instead. Keyed on the look in the selector because stylelint's `opacity` allowlist
       takes no `var()`; `:where()` keeps the rule's specificity. */
    :global(:where(:root:not([data-look='windows'])) .radio-item[data-disabled]) {
        opacity: 0.5;
    }

    :global(.radio-control) {
        width: var(--spacing-radio-size);
        height: var(--spacing-radio-size);
        min-width: var(--spacing-radio-size);
        min-height: var(--spacing-radio-size);
        border: var(--spacing-radio-border) solid var(--color-radio-border);
        border-radius: var(--radius-full);
        background: var(--color-radio-bg);
        flex-shrink: 0;
        transition: all var(--transition-base);
    }

    /* The Safari 15.4 floor's guard (`scripts/vite-webkit-css-floor.ts`): every engine with contrast
       themes reads it. */
    @supports (forced-color-adjust: none) {
        :global(.radio-control) {
            forced-color-adjust: var(--color-forced-adjust);
        }
    }

    :global(.radio-item:hover .radio-control[data-state='unchecked']) {
        border-color: var(--color-radio-border-hover);
        background: var(--color-radio-bg-hover);
    }

    /* Nudges the dot onto the label's optical middle when a description stacks
       below it, which `align-items: flex-start` alone leaves a hair high. */
    :global(.radio-item[data-described] .radio-control) {
        margin-top: var(--spacing-xxs);
    }

    /* The checked dot is the fill inside an inset ring: the house look rings an accent dot
       with the page color, Windows rings an on-accent dot with the accent. */
    :global(.radio-control[data-state='checked']) {
        border-color: var(--color-radio-checked-border);
        background: var(--color-radio-checked-bg);
        box-shadow: var(--shadow-radio-checked);
    }

    :global(.radio-item:hover .radio-control[data-state='checked']) {
        border-color: var(--color-radio-checked-border-hover);
        background: var(--color-radio-checked-bg-hover);
        box-shadow: var(--shadow-radio-checked-hover);
    }

    /* Ark UI uses data-focus attribute when the hidden input is focused */
    :global(.radio-item[data-focus]) {
        outline: 2px solid var(--color-focus-ring);
        outline-offset: var(--spacing-focus-ring-control-offset);
        border-radius: var(--radius-sm);
        box-shadow: var(--shadow-focus-ring-control);
    }

    :global(.radio-text) {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xxs);
    }

    /* `display: contents` is the default on purpose: a group with no `itemFooter` keeps
       `.radio-row` as the flex / grid item, so the wrapper costs no layout at all. Only a
       group that asked for per-option footers becomes a real box, holding the option and
       its footer as one unit through the column or the grid. */
    .radio-cell {
        display: contents;
    }

    /* The gap is not decoration: the option's focus ring is drawn OUTSIDE its box
       (`outline-offset`), so a footer sitting flush under the row gets the ring painted
       across it. `--spacing-sm` clears the ring's full reach with room to spare. */
    .radio-cell.has-footer {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
        min-width: 0;
    }

    /* Label plus `itemInline` content on one line. `baseline` so a badge sits on the
       label's own baseline rather than the middle of its box; the run wraps as a whole
       when the row runs out of width. */
    .radio-label-line {
        display: flex;
        align-items: baseline;
        flex-wrap: wrap;
        gap: var(--spacing-sm);
        min-width: 0;
    }

    .radio-label {
        color: var(--color-radio-label);
        font-size: var(--font-radio-label-size);
    }

    .radio-description {
        color: var(--color-text-tertiary);
        font-size: var(--font-size-sm);
    }
</style>
