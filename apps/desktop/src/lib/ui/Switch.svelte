<script lang="ts">
    import { Switch } from '@ark-ui/svelte/switch'
    import type { Snippet } from 'svelte'

    /**
     * The house switch: a thin, presentational wrapper over Ark UI's `Switch` so the macOS-y
     * track-and-thumb look lives in one place. `SettingSwitch` wraps this and adds the settings
     * registry wiring; feature code binds it directly.
     *
     * Bind the state: `<Switch bind:checked={value} />`. Pass `children` to render an inline label
     * to the right of the track; omit it for a bare track (rows that own their label).
     *
     * Switch vs `Checkbox`: a switch reads as "this is on/off right now", a checkbox as "this
     * option is selected". Both are fine in a form; pick by which sentence the control tells.
     *
     * Any extra `data-*` attribute passes straight through to the hidden `<input>`
     * (`<Switch data-test="media-net-optin" data-volume-id={id} />`). The input is the element
     * tests query and click: Ark marks the styled track `aria-hidden`, so a hook on the track
     * would sit on decoration. Nothing else spreads through, so the API stays a fixed shape.
     */
    interface Props {
        checked?: boolean
        disabled?: boolean
        id?: string
        /** Accessible name when there's no visible `children` label. */
        ariaLabel?: string
        /** Id of an element that explains the control, for example why it's disabled. */
        ariaDescribedBy?: string
        /**
         * Annotate the parameter at inline call sites (`(next: boolean) => …`): svelte2tsx
         * doesn't contextually type an arrow passed to a component prop, so an unannotated
         * one is `any` and trips `@typescript-eslint/no-unsafe-argument`.
         */
        onCheckedChange?: (checked: boolean) => void
        children?: Snippet
        /** Test/automation hooks; they land on the hidden `<input>`, not the track. */
        [dataAttribute: `data-${string}`]: string | undefined
    }

    /* eslint-disable prefer-const -- $bindable() requires `let` destructuring */
    let {
        checked = $bindable(false),
        disabled = false,
        id,
        ariaLabel,
        ariaDescribedBy,
        onCheckedChange,
        children,
        ...dataAttributes
    }: Props = $props()
    /* eslint-enable prefer-const */
</script>

<Switch.Root
    class="switch-root"
    {checked}
    onCheckedChange={(details) => {
        checked = details.checked
        onCheckedChange?.(checked)
    }}
    {disabled}
    {id}
>
    <Switch.Control class="switch-control">
        <Switch.Thumb class="switch-thumb" />
    </Switch.Control>
    {#if children}
        <Switch.Label class="switch-label">{@render children()}</Switch.Label>
    {/if}
    <!-- Both attributes belong on the INPUT, which is the thing assistive tech sees.
         `role="switch"`: Ark ships a bare `input[type=checkbox]`, so without it a
         screen reader says "checkbox", not "switch, on".
         `aria-label`: Ark points the input's `aria-labelledby` at `Switch.Label`, which
         doesn't exist when the caller passes no `children` — a dangling reference
         leaves the control with NO accessible name. `aria-labelledby` still wins when
         a visible label IS rendered, so passing both is safe. -->
    <Switch.HiddenInput role="switch" aria-label={ariaLabel} aria-describedby={ariaDescribedBy} {...dataAttributes} />
</Switch.Root>

<style>
    :global(.switch-root) {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-sm);
        cursor: default;
        /* Contain Ark's `HiddenInput`. Ark hides it sr-only style (`position:
           absolute` with `clip`, no `top`/`left`), so it resolves against the
           nearest positioned ancestor. Without this, that's the window shell,
           and the input's flow position (which can sit below the fold) makes
           the shell secretly scrollable; focusing it on click scrolls the whole
           window under the traffic lights. Keeping it local lets the real
           scroller clip it. */
        position: relative;
    }

    :global(.switch-control) {
        display: inline-flex;
        align-items: center;
        flex-shrink: 0;
        width: var(--spacing-switch-width);
        height: 20px;
        background: var(--color-switch-track);
        /* The house track has no edge (`none`); Windows' is a 1px stroke, a whole `border`
           shorthand per state so the house look keeps no border at all. */
        border: var(--color-switch-border);
        border-radius: var(--radius-full);
        padding: var(--spacing-switch-padding);
        cursor: default;
        forced-color-adjust: var(--color-forced-adjust);
        transition: background-color var(--transition-base);
    }

    :global(.switch-control:hover) {
        background-color: var(--color-switch-track-hover);
        border: var(--color-switch-border-hover);
    }

    :global(.switch-control[data-state='checked']) {
        background: var(--color-switch-track-on);
        border: var(--color-switch-border-on);
    }

    /* Disabled re-points the state tokens at their disabled twins (the house twins equal
       what it painted before; it dims with opacity instead, below). */
    :global(.switch-control[data-disabled]) {
        --color-switch-track: var(--color-switch-track-disabled);
        --color-switch-track-hover: var(--color-switch-track-disabled);
        --color-switch-border: var(--color-switch-border-disabled);
        --color-switch-border-hover: var(--color-switch-border-disabled);
        --color-switch-track-on: var(--color-switch-track-on-disabled);
        --color-switch-track-on-hover: var(--color-switch-track-on-disabled-hover);
        --color-switch-border-on: var(--color-switch-border-on-disabled);
        --color-switch-border-on-hover: var(--color-switch-border-on-disabled);
        --color-switch-thumb: var(--color-switch-thumb-disabled);
        --color-switch-thumb-hover: var(--color-switch-thumb-disabled);
        --color-switch-thumb-on: var(--color-switch-thumb-on-disabled);
        --color-switch-thumb-on-hover: var(--color-switch-thumb-on-disabled);

        cursor: not-allowed;
    }

    /* The dim is the house look's disabled state; Windows paints WinUI's disabled colors
       instead. Keyed on the look in the selector because stylelint's `opacity` allowlist
       takes no `var()`; `:where()` keeps the rule's specificity. */
    :global(:where(:root:not([data-look='windows'])) .switch-control[data-disabled]) {
        opacity: 0.5;
    }

    :global(.switch-root:has(.switch-control[data-disabled]) .switch-label) {
        color: var(--color-control-label-disabled);
    }

    /* The house thumb is literal white in both themes, so it reads against the accent
       track; Windows' knob is a text-colored dot off and the on-accent color on. */
    :global(.switch-thumb) {
        width: var(--spacing-switch-thumb-size);
        height: var(--spacing-switch-thumb-size);
        background: var(--color-switch-thumb);
        border-radius: var(--radius-full);
        box-shadow: var(--shadow-switch-thumb);
    }

    :global(.switch-control:hover .switch-thumb) {
        background: var(--color-switch-thumb-hover);
    }

    :global(.switch-control[data-state='checked']:hover .switch-thumb) {
        background: var(--color-switch-thumb-on-hover);
    }

    /* The thumb slides only when motion is welcome; under Reduce motion it jumps. */
    @media (prefers-reduced-motion: no-preference) {
        :global(.switch-thumb) {
            transition: transform var(--transition-base);
        }
    }

    :global(.switch-control[data-state='checked'] .switch-thumb) {
        background: var(--color-switch-thumb-on);
        transform: translateX(var(--spacing-switch-travel));
    }

    :global(.switch-control[data-state='checked']:hover) {
        background: var(--color-switch-track-on-hover);
        border: var(--color-switch-border-on-hover);
    }

    :global(.switch-label) {
        font-size: var(--font-size-md);
        cursor: default;
    }

    /* Ark UI uses data-focus attribute when the hidden input is focused */
    :global(.switch-control[data-focus]) {
        outline: 2px solid var(--color-focus-ring);
        outline-offset: var(--spacing-focus-ring-control-offset);
        box-shadow: var(--shadow-focus-ring-control);
    }
</style>
