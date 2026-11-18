<script lang="ts">
    import { onMount, type Snippet } from 'svelte'
    import { tooltip, type TooltipParam } from '#lib/tooltip/tooltip.js'
    import type { CommandId } from '#lib/commands/index.js'
    import ShortcutChip from './ShortcutChip.svelte'

    type Variant = 'primary' | 'secondary' | 'danger'
    type Size = 'regular' | 'mini'
    /** A fixed key (`"F2"`, `"S"`, `"⏎"`), or a command whose live binding shows (nothing while it has none). */
    type KeyHint = string | { commandId: CommandId }

    interface Props {
        variant?: Variant
        size?: Size
        disabled?: boolean
        /**
         * Blocked, not disabled: the button looks and reads unavailable (`aria-disabled`,
         * dimmed, `not-allowed` cursor) but stays focusable and still fires `onclick`, so
         * the handler can explain WHY instead of the press silently doing nothing. Reach
         * for this over `disabled` whenever the user can do something about the block, and
         * pair it with a `tooltipContent` naming the missing precondition. `disabled` stays right
         * for a button that's inert for reasons the user can't act on.
         */
        ariaDisabled?: boolean
        /** Tooltip on the button itself; a wrapper can't carry it, since focus doesn't bubble. */
        tooltipContent?: TooltipParam
        type?: 'button' | 'submit'
        onclick?: (e: MouseEvent) => void
        'aria-label'?: string
        /**
         * Id of the element that says WHICH thing this button acts on. For a button
         * repeated once per row, this is what keeps the name short ("Roll back") while
         * still telling a screen reader which row it belongs to.
         */
        'aria-describedby'?: string
        /**
         * The key that presses this button, drawn as a chip after the label in its own slot:
         * centered on the button, never wrapping away from the label, and `aria-hidden` (read
         * into the name it garbles it: "Skip S"). ❌ Never a `ShortcutChip` in `children`
         * (`cmdr/key-hint-via-button-prop`). Pair a fixed key with `aria-keyshortcuts`. ❌ No
         * `aria-label` beside a letter key: axe counts the on-screen chip as visible text, so no
         * label can contain it (WCAG 2.5.3); the visible word is the name.
         */
        keyHint?: KeyHint
        /** The key that presses this button, for a screen reader, in ARIA's spelling (`"Alt+C"`, `"S Enter"`). */
        'aria-keyshortcuts'?: string
        /**
         * For a button that shows and hides a region (a disclosure): the pair tells a screen
         * reader the button opens something, which one, and whether it's open now.
         */
        'aria-expanded'?: boolean
        'aria-controls'?: string
        /**
         * `-1` takes the button out of the Tab order while it stays clickable and focusable from
         * code: for a composite widget (a grid row) whose arrow keys reach its buttons instead.
         */
        tabindex?: -1 | 0
        /**
         * Focus this button after mount. Uses `requestAnimationFrame` so it lands
         * after a parent `ModalDialog` has settled its own mount-time focus (the
         * dialog leaves the scrim unfocused once something inside owns focus).
         */
        autoFocus?: boolean
        /**
         * The dialog's default button: what Return presses (`ModalDialog`, per the platform's
         * `enterActivates`), announced to screen readers as `Enter`. At most one per dialog, and
         * none where people should read before answering. A `danger` default makes Return on any
         * other button do nothing on the Mac. DETAILS § Platform conventions.
         */
        isDefault?: boolean
        children: Snippet
    }

    const {
        variant = 'secondary',
        size = 'regular',
        disabled = false,
        ariaDisabled = false,
        tooltipContent,
        type = 'button',
        onclick,
        'aria-label': ariaLabel,
        'aria-describedby': ariaDescribedby,
        keyHint,
        'aria-keyshortcuts': ariaKeyshortcuts,
        'aria-expanded': ariaExpanded,
        'aria-controls': ariaControls,
        tabindex,
        autoFocus = false,
        isDefault = false,
        children,
    }: Props = $props()

    /** `data-dialog-default` (`dialog-keys.ts`), where Return finds the default button. */
    const defaultMark = $derived(isDefault ? '' : undefined)
    /** The default button's Return, beside any key the caller names. */
    const keyshortcuts = $derived(
        isDefault ? (ariaKeyshortcuts ? `${ariaKeyshortcuts} Enter` : 'Enter') : ariaKeyshortcuts,
    )

    let buttonEl: HTMLButtonElement | undefined
    onMount(() => {
        if (!autoFocus) return
        requestAnimationFrame(() => {
            buttonEl?.focus()
        })
    })
</script>

<!-- eslint-disable-next-line cmdr/key-hint-via-button-prop -- this IS the key hint's slot -->
<button
    bind:this={buttonEl}
    {type}
    class="btn btn-{variant} btn-{size}"
    class:btn-keyed={keyHint !== undefined}
    {disabled}
    aria-disabled={ariaDisabled ? 'true' : undefined}
    {onclick}
    aria-label={ariaLabel}
    aria-describedby={ariaDescribedby}
    aria-keyshortcuts={keyshortcuts}
    data-dialog-default={defaultMark}
    aria-expanded={ariaExpanded}
    aria-controls={ariaControls}
    {tabindex}
    use:tooltip={tooltipContent}
>
    {#if keyHint === undefined}
        {@render children()}
    {:else}
        <span class="btn-label">{@render children()}</span>
        <!-- No whitespace inside: an unbound command draws nothing, and `:empty` then drops the slot. -->
        <span class="btn-key-hint" aria-hidden="true"
            >{#if typeof keyHint === 'string'}<ShortcutChip key={keyHint} size="sm" />{:else}<ShortcutChip
                    commandId={keyHint.commandId}
                    clickable={false}
                    size="sm"
                />{/if}</span
        >
    {/if}
</button>

<style>
    /* The look is the `--*-button-*` tokens (`app-controls.css`): the house capsule, the
       shape macOS gives an alert's action buttons (`--radius-full` rather than a percentage,
       which would go oval on a wide button), or Windows 11's.
       A label never wraps, the way a macOS button's doesn't: the button grows instead, and
       its container decides where it goes (a toast widens, `ToastActions`). `overflow-wrap`
       resets an inherited `anywhere`, which would otherwise let a label break mid-word. */
    .btn {
        white-space: nowrap;
        overflow-wrap: normal;
        font-weight: 500;
        line-height: var(--font-line-height-normal);
        border-radius: var(--radius-button);
        transition: all var(--transition-base);
    }

    /* The Safari 15.4 floor's guard (`scripts/vite-webkit-css-floor.ts`): every engine with contrast
       themes reads it. */
    @supports (forced-color-adjust: none) {
        .btn {
            forced-color-adjust: var(--color-forced-adjust);
        }
    }

    .btn:disabled {
        cursor: not-allowed;
        pointer-events: none;
    }

    /* Blocked: same read as disabled, but pointer events stay live (and the button stays
       focusable) so the press reaches `onclick` and the handler can say what's missing. */
    .btn[aria-disabled='true'] {
        cursor: not-allowed;
    }

    /* The house look dims a disabled button; Windows paints WinUI's disabled colors instead
       (the `-disabled` tokens), so the dim stays off there. Keyed on the look in the selector,
       because stylelint's `opacity` allowlist takes no `var()`; `:where()` keeps the
       specificity of the rules above. */
    /* WinUI's Button is FontWeight Normal; the house label is medium. Keyed on the look in the
       selector, like the dim below, because stylelint's `font-weight` allowlist takes no `var()`. */
    :global(:where(:root[data-look='windows'])) .btn {
        font-weight: 400;
    }

    :global(:where(:root:not([data-look='windows']))) .btn:disabled,
    :global(:where(:root:not([data-look='windows']))) .btn[aria-disabled='true'] {
        opacity: 0.4;
    }

    .btn:focus-visible {
        outline: 2px solid var(--color-focus-ring);
        outline-offset: 1px;
        box-shadow: var(--shadow-focus-ring-button);
    }

    /* A key hint gets its own flex slot, so the chip centers on the button's cross axis
       (inline, it sat on the text's baseline and read low) and never wraps under the
       label: the button grows instead. */
    .btn-keyed {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: var(--spacing-sm);
    }

    .btn-label,
    .btn-key-hint {
        display: inline-flex;
        align-items: center;
    }

    /* A command with no binding draws no chip, and its empty slot mustn't add a gap. */
    .btn-key-hint:empty {
        display: none;
    }

    /* === Size: regular === */
    .btn-regular {
        padding: var(--spacing-button-padding);
        font-size: var(--font-size-md);
    }

    /* === Size: mini === */
    .btn-mini {
        /* stylelint-disable-next-line declaration-property-value-disallowed-list -- Mini button height target: 22px */
        padding: 3px 12px;
        font-size: var(--font-size-sm);
    }

    /* === Variant: primary === */
    /* Every variant carries a 1px border so all share one box: without it a primary sat 2px
       narrower and shorter than its secondary and danger neighbors, and mixed rows misaligned.
       Transparent, so the accent fill (painted under the border, `background-clip: border-box`)
       reads edge to edge. In forced-colors mode the browser paints it in the system button
       color on purpose: that's the one outline a primary has there, like its siblings.
       Windows paints its own contrast-theme pairs instead (`--color-forced-adjust`).
       Each variant's edge is three tokens (sides, top, bottom), since WinUI's elevation
       border is darker along one edge; the house look gives all three one color. */
    .btn-primary {
        background: var(--color-button-primary-bg);
        color: var(--color-button-primary-fg);
        border: 1px solid var(--color-button-primary-border);
        border-top-color: var(--color-button-primary-border-top);
        border-bottom-color: var(--color-button-primary-border-bottom);
    }

    .btn-primary:hover:not(:disabled, [aria-disabled='true']) {
        background: var(--color-button-primary-bg-hover);
        color: var(--color-button-primary-fg-hover);
        border-color: var(--color-button-primary-border-hover);
        border-bottom-color: var(--color-button-primary-border-bottom);
    }

    .btn-primary:hover:active:not(:disabled, [aria-disabled='true']) {
        background: var(--color-button-primary-bg-pressed);
        color: var(--color-button-primary-fg-pressed);
        border-color: var(--color-button-primary-border-pressed);
    }

    .btn-primary:is(:disabled, [aria-disabled='true']) {
        background: var(--color-button-primary-bg-disabled);
        color: var(--color-button-primary-fg-disabled);
        border-color: var(--color-button-primary-border-disabled);
    }

    /* === Variant: secondary (WinUI's standard Button) === */
    .btn-secondary {
        background: var(--color-button-secondary-bg);
        color: var(--color-button-secondary-fg);
        border: 1px solid var(--color-button-secondary-border);
        border-top-color: var(--color-button-secondary-border-top);
        border-bottom-color: var(--color-button-secondary-border-bottom);
    }

    .btn-secondary:hover:not(:disabled, [aria-disabled='true']) {
        background: var(--color-button-secondary-bg-hover);
        color: var(--color-button-secondary-fg-hover);
        border-color: var(--color-button-secondary-border-hover);
        border-top-color: var(--color-button-secondary-border-top);
        border-bottom-color: var(--color-button-secondary-border-bottom);
    }

    .btn-secondary:hover:active:not(:disabled, [aria-disabled='true']) {
        background: var(--color-button-secondary-bg-pressed);
        color: var(--color-button-secondary-fg-pressed);
        border-color: var(--color-button-secondary-border-pressed);
    }

    .btn-secondary:is(:disabled, [aria-disabled='true']) {
        background: var(--color-button-secondary-bg-disabled);
        color: var(--color-button-secondary-fg-disabled);
        border-color: var(--color-button-secondary-border-disabled);
    }

    /* === Variant: danger (no WinUI twin: the standard Button in the critical color) === */
    .btn-danger {
        background: var(--color-button-danger-bg);
        color: var(--color-button-danger-fg);
        border: 1px solid var(--color-button-danger-border);
        border-top-color: var(--color-button-danger-border-top);
        border-bottom-color: var(--color-button-danger-border-bottom);
    }

    .btn-danger:hover:not(:disabled, [aria-disabled='true']) {
        background: var(--color-button-danger-bg-hover);
        border-color: var(--color-button-danger-border-hover);
        border-top-color: var(--color-button-danger-border-top);
        border-bottom-color: var(--color-button-danger-border-bottom);
    }

    .btn-danger:hover:active:not(:disabled, [aria-disabled='true']) {
        background: var(--color-button-danger-bg-pressed);
        color: var(--color-button-danger-fg-pressed);
        border-color: var(--color-button-danger-border-pressed);
    }

    .btn-danger:is(:disabled, [aria-disabled='true']) {
        background: var(--color-button-danger-bg-disabled);
        color: var(--color-button-danger-fg-disabled);
        border-color: var(--color-button-danger-border-disabled);
    }
</style>
