<script lang="ts">
    import { Checkbox } from '@ark-ui/svelte/checkbox'
    import type { Snippet } from 'svelte'
    import Icon from '#lib/ui/Icon.svelte'

    /**
     * The house checkbox. A thin, presentational wrapper over Ark UI's `Checkbox` so the macOS-y
     * look lives in one place. Unlike a raw `<input type="checkbox">` it does NOT gray out when the
     * window loses focus, and it themes through the design tokens.
     *
     * Bind the state: `<Checkbox bind:checked={value} />`. Pass `children` to render an inline label
     * to the right of the box; omit it for a bare box (list rows, dense grids that own their label).
     */
    interface Props {
        checked?: boolean
        disabled?: boolean
        /** Renders the mixed (dash) state; overrides `checked` visually while set. */
        indeterminate?: boolean
        id?: string
        /** Accessible name when there's no visible `children` label. */
        ariaLabel?: string
        /** Id of an element that explains the control, for example why it's disabled. */
        ariaDescribedBy?: string
        /**
         * Marks the box as a required choice (`aria-required` on the control). A visible
         * asterisk is decoration only; this is what a screen reader announces, so set it
         * whenever the surrounding UI marks the field required.
         */
        required?: boolean
        onCheckedChange?: (checked: boolean) => void
        /**
         * `-1` takes the box out of the Tab order while it stays clickable: for a row in a
         * composite widget whose own key (Space) checks it instead.
         */
        tabindex?: -1 | 0
        children?: Snippet
    }

    /* eslint-disable prefer-const -- $bindable() requires `let` destructuring */
    let {
        checked = $bindable(false),
        disabled = false,
        indeterminate = false,
        id,
        ariaLabel,
        ariaDescribedBy,
        required = false,
        onCheckedChange,
        tabindex,
        children,
    }: Props = $props()
    /* eslint-enable prefer-const */

    // Ark's `checked` accepts `boolean | 'indeterminate'`.
    const checkedState = $derived(indeterminate ? ('indeterminate' as const) : checked)
</script>

<Checkbox.Root
    class="checkbox-root"
    checked={checkedState}
    onCheckedChange={(details) => {
        checked = details.checked === true
        onCheckedChange?.(checked)
    }}
    {disabled}
    {id}
>
    <Checkbox.Control class="checkbox-control">
        <Checkbox.Indicator class="checkbox-indicator">
            <span class="checkbox-check"><Icon name="check" size={12} aria-hidden="true" /></span>
            <span class="checkbox-dash" aria-hidden="true"></span>
        </Checkbox.Indicator>
    </Checkbox.Control>
    {#if children}
        <Checkbox.Label class="checkbox-label">{@render children()}</Checkbox.Label>
    {/if}
    <!-- `aria-label` belongs on the INPUT: Ark points its `aria-labelledby` at
         `Checkbox.Label`, which doesn't exist when the caller passes no `children`, and
         a dangling reference leaves the control with NO accessible name. `aria-label`
         on the wrapping `<label>` names the label, not the control. `aria-labelledby`
         still wins when a visible label IS rendered, so passing both is safe. -->
    <Checkbox.HiddenInput
        aria-label={ariaLabel}
        aria-describedby={ariaDescribedBy}
        aria-required={required ? 'true' : undefined}
        {tabindex}
    />
</Checkbox.Root>

<style>
    /* Ark's `Root` is a `<label>`, `display: inline` by default, which baseline-aligns
       the box against the label text and leaves the box sitting low. Flex + `center`
       puts the box on the label's optical middle; the gap lives here too, since the
       label span is the box's SIBLING, not its container. */
    :global(.checkbox-root) {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-sm);
        cursor: default;
        /* Contain Ark's `HiddenInput` so it resolves against this control, not
           the window shell. See `Switch.svelte`'s `.switch-root` for the full
           why (the shell-scroll-under-traffic-lights bug). */
        position: relative;
    }

    /* A 16px box is a small pointer target, so a labeled checkbox grows its clickable
       band all round. The padding is cancelled by an equal negative margin, so nothing
       moves. A BARE box (dense list rows, grids) keeps its exact footprint: there, an
       overhang would reach into the neighbouring row and steal its clicks. */
    :global(.checkbox-root:has(.checkbox-label)) {
        padding: var(--spacing-xs);
        margin: calc(-1 * var(--spacing-xs));
    }

    :global(.checkbox-control) {
        flex-shrink: 0;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: var(--spacing-checkbox-size);
        height: var(--spacing-checkbox-size);
        background: var(--color-checkbox-bg);
        border: 1px solid var(--color-checkbox-border);
        border-radius: var(--radius-checkbox);
        cursor: default;
        transition:
            background-color var(--transition-base),
            border-color var(--transition-base);
    }

    /* The Safari 15.4 floor's guard (`scripts/vite-webkit-css-floor.ts`): every engine with contrast
       themes reads it. */
    @supports (forced-color-adjust: none) {
        :global(.checkbox-control) {
            forced-color-adjust: var(--color-forced-adjust);
        }
    }

    :global(.checkbox-control[data-state='checked']),
    :global(.checkbox-control[data-state='indeterminate']) {
        background: var(--color-checkbox-checked-bg);
        border-color: var(--color-checkbox-checked-bg);
    }

    :global(.checkbox-control[data-state='checked']:hover),
    :global(.checkbox-control[data-state='indeterminate']:hover) {
        background: var(--color-checkbox-checked-bg-hover);
        border-color: var(--color-checkbox-checked-bg-hover);
    }

    /* Hover must push the outline FURTHER from the surface, so the house look keys it off
       a text token: `--color-border-strong` is fainter than `--color-control-border` and
       would make hovering look like a disabled state. */
    :global(.checkbox-control:hover) {
        background-color: var(--color-checkbox-bg-hover);
        border-color: var(--color-checkbox-border-hover);
    }

    :global(.checkbox-control[data-state='unchecked']:hover:active) {
        background-color: var(--color-checkbox-bg-pressed);
        border-color: var(--color-checkbox-border-pressed);
    }

    :global(.checkbox-control[data-state='checked']:hover:active),
    :global(.checkbox-control[data-state='indeterminate']:hover:active) {
        background: var(--color-checkbox-checked-bg-pressed);
        border-color: var(--color-checkbox-checked-bg-pressed);
    }

    /* Disabled re-points the state tokens at their disabled twins, so every rule above
       paints the disabled look without a specificity contest with `:hover`. The house
       twins equal what each state painted before (it dims with opacity instead, below). */
    :global(.checkbox-control[data-disabled]) {
        --color-checkbox-bg: var(--color-checkbox-bg-disabled);
        --color-checkbox-bg-hover: var(--color-checkbox-bg-disabled);
        --color-checkbox-bg-pressed: var(--color-checkbox-bg-disabled);
        --color-checkbox-border: var(--color-checkbox-border-disabled);
        --color-checkbox-border-hover: var(--color-checkbox-border-disabled-hover);
        --color-checkbox-border-pressed: var(--color-checkbox-border-disabled-hover);
        --color-checkbox-checked-bg: var(--color-checkbox-checked-bg-disabled);
        --color-checkbox-checked-bg-hover: var(--color-checkbox-checked-bg-disabled-hover);
        --color-checkbox-checked-bg-pressed: var(--color-checkbox-checked-bg-disabled-hover);
        --color-checkbox-glyph: var(--color-checkbox-glyph-disabled);

        cursor: not-allowed;
    }

    /* The dim is the house look's disabled state; Windows paints WinUI's disabled colors
       (the twins above) instead. Keyed on the look in the selector because stylelint's
       `opacity` allowlist takes no `var()`; `:where()` keeps the rule's specificity. */
    :global(:where(:root:not([data-look='windows'])) .checkbox-control[data-disabled]) {
        opacity: 0.5;
    }

    :global(.checkbox-root:has(.checkbox-control[data-disabled]) .checkbox-label) {
        color: var(--color-control-label-disabled);
    }

    /* ❌ Never let the glyphs decide this box's size, and ❌ never hide one with
       `display: none`. The root is inline-flex, so the line it sits on takes its
       baseline from the first flex item's first child — the indicator. An empty
       indicator has no baseline to give and the browser synthesizes one from the
       border box instead, which sat ~6px off: the whole control (and any prose
       around it) hopped every time the user ticked the box. A fixed indicator with
       both glyphs absolutely centred inside makes all three states one layout.
       `display: block` is load-bearing: Ark puts `hidden` on the indicator while
       unchecked, and the UA's `[hidden] { display: none }` would drop it from layout
       and bring the ~2px hop back. Pinned by `Checkbox.svelte.test.ts`. */
    :global(.checkbox-indicator) {
        display: block;
        position: relative;
        width: 12px;
        height: 12px;
        color: var(--color-checkbox-glyph);
    }

    /* Ark marks the indicator `hidden` whenever the box isn't CHECKED, indeterminate
       included (it only shows for indeterminate on an `indeterminate` indicator), so the
       UA's `display: none` swallowed the dash and a mixed box rendered as a blank accent
       square. The glyphs' own visibility rules above decide what shows, in every state. */
    :global(.checkbox-indicator[hidden]) {
        display: block;
    }

    .checkbox-check,
    .checkbox-dash {
        position: absolute;
        display: flex;
        align-items: center;
        justify-content: center;
        inset: 0;
    }

    /* Unchecked reads as an empty square. Ark keeps the indicator mounted in every
       state, so hide the glyph ourselves instead of relying on it to unmount. */
    :global(.checkbox-control[data-state='unchecked']) .checkbox-check,
    :global(.checkbox-control[data-state='indeterminate']) .checkbox-check {
        visibility: hidden;
    }

    /* Indeterminate: hide the check, show a horizontal bar. */
    .checkbox-dash {
        visibility: hidden;
    }

    .checkbox-dash::before {
        content: '';
        width: 8px;
        height: 2px;
        border-radius: var(--radius-xs);
        background: var(--color-checkbox-glyph);
    }

    :global(.checkbox-control[data-state='indeterminate']) .checkbox-dash {
        visibility: visible;
    }

    :global(.checkbox-label) {
        font-size: var(--font-size-md);
        cursor: default;
    }

    /* Ark UI uses data-focus attribute when the hidden input is focused */
    :global(.checkbox-control[data-focus]) {
        outline: 2px solid var(--color-focus-ring);
        outline-offset: var(--spacing-focus-ring-control-offset);
        box-shadow: var(--shadow-focus-ring-control);
    }
</style>
