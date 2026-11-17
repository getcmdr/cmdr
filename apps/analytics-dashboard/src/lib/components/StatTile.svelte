<!--
  One headline number: a label with its (i), the value big, and the change against the comparison window
  under it, colored green or red. `unavailable` replaces the value with a short reason ("Not available
  yet"); a null value or change reads as a dash, never a zero.
-->
<script lang="ts">
    import type { Snippet } from 'svelte'
    import type { ChangeTone } from '#lib/format.js'
    import { changeToneClass } from '#lib/format.js'
    import InfoPopover from './InfoPopover.svelte'

    interface Props {
        label: string
        info: Snippet
        /** The formatted value, or null for a dash. */
        value: string | null
        /** The formatted change, or null for a dash. */
        change: string | null
        /** The change's color, from `changeTone` (neutral when it prints as zero). */
        tone: ChangeTone
        /** What the change compares against, like "vs. 30 days earlier". */
        comparison: string
        unavailable?: string | null
    }

    const { label, info, value, change, tone, comparison, unavailable = null }: Props = $props()

    const changeClass = $derived(changeToneClass[tone])
</script>

<div class="min-w-0 rounded-lg border border-border-subtle bg-surface-elevated px-4 py-3">
    <!-- A div, not a <p>: the popover's panel holds <p>s and <ul>s. -->
    <div class="flex items-center gap-1.5 text-xs text-text-tertiary">
        {label}
        <InfoPopover topic={label}>{@render info()}</InfoPopover>
    </div>
    {#if unavailable}
        <p class="mt-2 text-sm text-text-tertiary">{unavailable}</p>
    {:else}
        <p class="mt-1 text-2xl font-bold text-text-primary tabular-nums">{value ?? '–'}</p>
        <p class="text-xs text-text-tertiary">
            <span class="tabular-nums {changeClass}">{change ?? '–'}</span>
            {comparison}
        </p>
    {/if}
</div>
