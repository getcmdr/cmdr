<script lang="ts">
    import type { Snippet } from 'svelte'
    import {
        getSettingDefinition,
        isModified,
        resetSetting,
        onSpecificSettingChange,
        type SettingId,
    } from '#lib/settings/index.js'
    import { getMatchIndicesForLabel, highlightMatches } from '#lib/settings/settings-search.js'
    import { disabledNoteId, settingAnchorId } from '#lib/settings/settings-window.js'
    import { tooltip } from '#lib/tooltip/tooltip.js'
    import Icon from '#lib/ui/Icon.svelte'
    import InfoTip from '#lib/ui/InfoTip.svelte'
    import { onMount } from 'svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { isSettingLocked } from '#lib/managed-policy/managed-policy.svelte.js'
    import { registerSectionRow } from './section-rows.svelte'
    import { placeDescription } from './description-placement'

    interface Props {
        id: SettingId
        label: string
        /**
         * The full description. Renders where the registry's `descriptionPlacement` says: under
         * the label (`inline`), or behind an ⓘ after the label (`tip`, with the registry's
         * `summary` under the label when it has one). `''` renders nothing.
         */
        description: string
        disabled?: boolean
        disabledReason?: string
        /**
         * A sentence explaining why the row is disabled and how to enable it, shown under the whole
         * row (with an info glyph) only while `disabled`. It stays outside the label column so it
         * keeps full contrast while the rest of the row greys out. Point the control's
         * `aria-describedby` at `disabledNoteId(id)`.
         */
        disabledNote?: string
        requiresRestart?: boolean
        /** When true, label and control each take 50% width for consistent vertical alignment across rows. */
        split?: boolean
        searchQuery?: string
        children: Snippet
        /**
         * A rich inline description (links, say), in place of `description`. Always renders under
         * the label, whatever the registry's placement.
         */
        descriptionContent?: Snippet
        /**
         * Rendered right after the label, inside the label wrapper, ahead of the reset pip
         * and the badges. For a small adornment that belongs to the label rather than the
         * control: an `<InfoTip>` carrying the long version of the description, say.
         */
        labelTrailing?: Snippet
    }

    const {
        id,
        label,
        description,
        disabled: sectionDisabled = false,
        disabledReason: sectionDisabledReason,
        disabledNote: sectionDisabledNote,
        requiresRestart = false,
        split = false,
        searchQuery = '',
        children,
        descriptionContent,
        labelTrailing,
    }: Props = $props()

    // A setting the organization manages renders locked whatever the section passed: disabled,
    // with ONE note (the managed one), no badge of the section's own, and no reset pip. The
    // primitive inside picks the lock up too (disabled, described by this row's note).
    const locked = $derived(isSettingLocked(id))
    const disabled = $derived(locked || sectionDisabled)
    const disabledReason = $derived(locked ? undefined : sectionDisabledReason)
    const disabledNote = $derived(locked ? tString('settings.managed.rowNote') : sectionDisabledNote)

    registerSectionRow(id)

    // Where the description goes is the registry's call (`descriptionInTip`), so a row opts in
    // without its section changing. Absent from the registry (a test stub, say) reads as inline.
    // A rich `descriptionContent` always stays inline, and stands in for the plain text.
    const definition = $derived(getSettingDefinition(id))
    const placed = $derived(
        placeDescription({
            placement: descriptionContent ? 'inline' : (definition?.descriptionPlacement ?? 'inline'),
            description: descriptionContent ? '' : description,
            summary: definition?.summary ?? '',
            searchQuery,
        }),
    )

    // Get highlighted label segments based on search query
    const labelSegments = $derived.by(() => {
        if (!searchQuery.trim()) {
            return [{ text: label, matched: false }]
        }
        const matchIndices = getMatchIndicesForLabel(searchQuery, id)
        return highlightMatches(label, matchIndices)
    })

    // Track modified state reactively by subscribing to changes
    let modified = $state(isModified(id))

    // Subscribe to setting changes to update modified state
    onMount(() => {
        return onSpecificSettingChange(id, () => {
            modified = isModified(id)
        })
    })

    function handleReset() {
        resetSetting(id)
    }
</script>

<!-- Every row carries its deep-link anchor, so a surface elsewhere in the app can send
     the user straight to one setting. `settingAnchorId` is the one definition of the
     convention; the `<label for>` above keeps naming the control, not this row. -->
<div class="setting-row" id={settingAnchorId(id)} class:disabled>
    <div class="setting-header" class:split>
        <div class="setting-label-column">
            <div class="setting-label-wrapper">
                <label class="setting-label" for={id}
                    >{#each labelSegments as segment, i (i)}{#if segment.matched}<mark class="search-highlight"
                                >{segment.text}</mark
                            >{:else}{segment.text}{/if}{/each}</label
                >
                {#if placed.tip}
                    <InfoTip label={tString('settings.control.moreAbout', { setting: label })} text={placed.tip} />
                {/if}
                {#if labelTrailing}{@render labelTrailing()}{/if}
                {#if modified && !locked}
                    <button
                        class="reset-button"
                        use:tooltip={tString('settings.control.resetToDefault')}
                        onclick={handleReset}
                        aria-label={tString('settings.control.resetToDefault')}
                    >
                        <Icon name="rotate-ccw" size={14} aria-hidden="true" />
                    </button>
                {/if}
                {#if disabled && disabledReason}
                    <span class="disabled-badge">{disabledReason}</span>
                {/if}
                {#if requiresRestart}
                    <span class="restart-badge">{tString('settings.control.restartRequired')}</span>
                {/if}
            </div>
            {#each placed.underLabel as line, i (i)}
                <p class="setting-description">{line}</p>
            {/each}
            {#if descriptionContent}
                <p class="setting-description">{@render descriptionContent()}</p>
            {/if}
        </div>
        <div class="setting-control">
            {@render children()}
        </div>
    </div>
    {#if disabled && disabledNote}
        <p class="setting-disabled-note" id={disabledNoteId(id)}>
            <span class="disabled-note-icon"><Icon name="info" size={14} aria-hidden="true" /></span>
            <span>{disabledNote}</span>
        </p>
    {/if}
</div>

<style>
    .setting-row {
        padding: var(--spacing-sm) 0;
        border-bottom: 1px solid var(--color-border-subtle);
    }

    .setting-row:last-child {
        border-bottom: none;
    }

    /* Dim the children, not the row: opacity can't be undone on a descendant, and the
       disabled note has to stay at full AA contrast to be readable. */
    .setting-row.disabled > :not(.setting-disabled-note) {
        opacity: 0.6;
    }

    .setting-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: var(--spacing-md);
    }

    .setting-header.split {
        display: grid;
        grid-template-columns: 1fr 1fr;
        align-items: center;
        gap: var(--spacing-md);
    }

    /* The label line plus every description line under it (a summary, an inline description, or
       a rich one). In a split row this is the left grid cell, so the text stays beside the label
       and the control instead of running under the whole row. */
    .setting-label-column {
        display: flex;
        flex-direction: column;
        min-width: 0;
    }

    .setting-description {
        margin: var(--spacing-xxs) 0 0;
        color: var(--color-text-secondary);
        font-size: var(--font-size-sm);
    }

    .setting-label-wrapper {
        display: flex;
        align-items: center;
        gap: var(--spacing-xs);
    }

    .reset-button {
        display: flex;
        align-items: center;
        justify-content: center;
        margin-left: var(--spacing-xs);
        padding: 0;
        background: none;
        border: none;
        color: var(--color-accent-text);
        cursor: default;
    }

    .reset-button:hover {
        /* Keep the a11y-safe accent-text color on hover; add underline for visual
           affordance instead of switching to the lighter `--color-accent-hover`
           which doesn't meet 4.5:1 on white. */
        text-decoration: underline;
    }

    .setting-label {
        font-weight: 500;
        color: var(--color-text-primary);
    }

    .disabled-badge,
    .restart-badge {
        font-size: var(--font-size-xs);
        padding: var(--spacing-xxs) var(--spacing-xs);
        border-radius: var(--radius-sm);
        font-weight: 500;
    }

    .disabled-badge {
        background: var(--color-bg-tertiary);
        color: var(--color-text-tertiary);
    }

    .restart-badge {
        background: var(--color-accent);
        color: var(--color-accent-fg);
        margin-left: var(--spacing-xs);
    }

    .setting-control {
        flex-shrink: 0;
    }

    .split .setting-control {
        /* Let controls stretch to fill the right column */
        min-width: 0;
        width: 100%;
    }

    .setting-disabled-note {
        display: flex;
        align-items: flex-start;
        gap: var(--spacing-xs);
        margin: var(--spacing-xs) 0 0;
        color: var(--color-text-secondary);
        font-size: var(--font-size-sm);
    }

    /* Centers the glyph on the first text line, however the note wraps. */
    .disabled-note-icon {
        display: flex;
        align-items: center;
        height: 1lh;
    }

    .search-highlight {
        background-color: var(--color-highlight);
        color: inherit;
        padding: 0 var(--spacing-xxs);
        border-radius: var(--radius-xs);
    }
</style>
