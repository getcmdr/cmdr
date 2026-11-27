<!--
    The dual-bar progress readout: a bytes bar and a count bar, each with its
    done/total, percent, and rate, over one time-left line shared by both.

    ONE widget serves both surfaces that show a running write operation — the
    copy/move/delete progress dialog and the operation queue's rows — so the two
    can't drift apart in what they show or how it reads. The numbers themselves
    are single-sourced elsewhere (`progress-readout.ts` for speed and ETA,
    `#lib/units` for the text); this file owns the layout only.

    Every readout cell is a FIXED-width column sized for its own worst case, so
    the bars' width follows the window and nothing shifts as digits come and go
    ("9 GB" → "10 GB", "(99%)" → "(100%)"). The time left gets its own row under
    the bars, which both keeps the bars wide and lets the estimate switch between
    "1h 8m left" and "56m 24s left" without moving anything above it.

    Under them, opposite the time left, how many files (or items) are left.

    BOTH densities label their bars. The two densities differ only in the size of
    the bar, the gaps, and the bar's floor.
-->
<script lang="ts">
    import ProgressBar from '#lib/ui/ProgressBar.svelte'
    import Size from '#lib/ui/Size.svelte'
    import { sizeDisplayText } from '#lib/ui/size-display.js'
    import Trans from '#lib/intl/Trans.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { calculatePercentage, formatNumber } from '#lib/file-explorer/selection/selection-info-utils.js'
    import { formatDuration, formatFilesPerSecond, seconds, type Seconds } from '#lib/units/index.js'
    import type { StallNotice, TransferWaitLine } from './transfer/transfer-stall'

    interface Props {
        bytesDone: number
        /** Zero (unknown total) hides the bytes row; the count row always shows. */
        bytesTotal: number
        filesDone: number
        filesTotal: number
        /** The session's `bytesPerSecondDisplay`, ❌ never a rate off the raw
         *  tick: the session is what decides when there's an honest number to
         *  show. `null` leaves the cell empty. */
        bytesPerSecond?: number | null
        /** The session's `filesPerSecondDisplay`, on the same terms. */
        filesPerSecond?: number | null
        /** The session's `etaSecondsDisplay`: smoothed, and absent whenever no
         *  countdown can be believed — which a pause and an open clash are NOT,
         *  since the work left over is what the person is deciding about.
         *  ❌ Never the raw backend value. */
        etaSeconds?: Seconds | null
        /** The backend's stall verdict; it displaces the countdown while set. */
        stall?: StallNotice | null
        /** A wait worth describing while bytes are on their way but none has
         *  landed, or while a volume that went away comes back (`waitLineFor`):
         *  it displaces the countdown too, and a stall outranks it. */
        waitLine?: TransferWaitLine | null
        /** The backend's `outgrewTotals`: the work outgrew the scan's count, so
         *  the totals were raised to what's done and there's no countdown. The
         *  line says so in the countdown's place; a stall and a wait outrank it. */
        outgrewTotals?: boolean
        /** The count row's noun: trash moves items, everything else files. */
        countKind?: 'files' | 'items'
        /** `compact` is a Transfers-window row, `comfortable` the dialog. */
        density?: 'comfortable' | 'compact'
    }

    const {
        bytesDone,
        bytesTotal,
        filesDone,
        filesTotal,
        bytesPerSecond = null,
        filesPerSecond = null,
        etaSeconds = null,
        stall = null,
        waitLine = null,
        outgrewTotals = false,
        countKind = 'files',
        density = 'comfortable',
    }: Props = $props()

    const isCompact = $derived(density === 'compact')

    const hasBytesRow = $derived(bytesTotal > 0)
    const bytePercent = $derived(calculatePercentage(bytesDone, bytesTotal))
    const filePercent = $derived(calculatePercentage(filesDone, filesTotal))

    /** Whether the count bar has a denominator to measure against.
     *
     *  A total of zero is the backend saying it hasn't decided yet (it's what a
     *  scanning operation reports on both axes), ❌ never "nothing to do". A
     *  fraction against it is a fraction against a number nobody has committed
     *  to, and it renders as a bar frozen at 0% beside "(0%)" on an operation
     *  that is moving. So the row keeps its count and drops the bar and the
     *  percentage until a denominator lands — the same answer the corner chip
     *  gives a scanning operation, and the same one `hasBytesRow` gives the size
     *  axis one line up. The callers' own phase gates should mean this never
     *  fires; it's here so a surface that forgets one can't invent a percentage. */
    const hasCountFraction = $derived(filesTotal > 0)

    /** A zero rate is the estimator saying nothing has moved yet, not a speed. */
    const byteRate = $derived(bytesPerSecond !== null && bytesPerSecond > 0 ? bytesPerSecond : null)
    /** `null` for a rate that rounds to zero, so the cell stays empty. The
     *  "files/s" marker is catalog copy, ❌ never a literal here: the byte rate
     *  one line up composes its own marker the same way, and a hardcoded one
     *  ships English to all thirteen locales. */
    const fileRate = $derived(filesPerSecond === null ? null : formatFilesPerSecond(filesPerSecond))
    const fileRateText = $derived(
        fileRate === null
            ? null
            : tString('fileOperations.shared.fileRate', { count: fileRate.value, rateText: fileRate.text }),
    )

    /** What each bar's `aria-valuetext` says: the amount printed beside it, so a screen
     *  reader hears "1.2 GB of 3 GB", not only "40 percent". */
    const sizeValueText = $derived(
        tString('fileOperations.transferProgress.sizeProgressValueText', {
            doneText: sizeDisplayText(bytesDone, true),
            totalText: sizeDisplayText(bytesTotal, true),
        }),
    )
    const countValueText = $derived(
        tString(
            countKind === 'items'
                ? 'fileOperations.transferProgress.itemProgressValueText'
                : 'fileOperations.transferProgress.fileProgressValueText',
            { count: filesTotal, doneText: formatNumber(filesDone), totalText: formatNumber(filesTotal) },
        ),
    )

    const countLabel = $derived(
        countKind === 'items'
            ? tString('fileOperations.transferProgress.progressItems')
            : tString('fileOperations.transferProgress.progressFiles'),
    )

    /** "1,312 files left", once there's a total to count against and while any are. */
    const leftText = $derived.by(() => {
        const left = filesTotal - filesDone
        if (!hasCountFraction || left <= 0) return null
        const key =
            countKind === 'items'
                ? 'fileOperations.transferProgress.itemsLeft'
                : 'fileOperations.transferProgress.filesLeft'
        return tString(key, { count: left, countText: formatNumber(left) })
    })

    /** The source's receive rate, when that's what the time line shows. It
     *  renders through a `<Size>` widget, so it can't be part of `timeText`. */
    const receivingRate = $derived(!stall && waitLine?.kind === 'receiving' ? waitLine.bytesPerSecond : null)

    /** A countdown we no longer believe is worse than no countdown, so a stalled
     *  transfer says how long it's been still instead, and one whose first file
     *  is still being opened says that. */
    const timeText = $derived.by(() => {
        if (stall) {
            return tString('fileOperations.transferProgress.stallNotice', {
                duration: formatDuration(seconds(stall.stillForSeconds)),
            })
        }
        if (waitLine?.kind === 'reconnecting') {
            return tString('fileOperations.transferProgress.waitingForVolume', { volume: waitLine.volumeName })
        }
        if (waitLine?.kind === 'opening') {
            return tString('fileOperations.transferProgress.openingSource', {
                duration: formatDuration(seconds(waitLine.forSeconds)),
            })
        }
        // Full bars over work that carries on (ERR-BGEKC): say why, where "0s left" was.
        if (outgrewTotals) return tString('fileOperations.transferProgress.outgrewCount')
        if (etaSeconds === null) return null
        return tString('fileOperations.transferProgress.etaRemaining', { duration: formatDuration(etaSeconds) })
    })
</script>

<div class="progress-readout" class:compact={isCompact}>
    {#if hasBytesRow}
        <span class="bar-label">{tString('fileOperations.transferProgress.progressBytes')}</span>
        <ProgressBar
            value={bytesDone / bytesTotal}
            size={isCompact ? 'sm' : 'md'}
            ariaLabel={tString('fileOperations.transferProgress.sizeProgressAria')}
            ariaValuetext={sizeValueText}
        />
        <span class="amount"><Size bytes={bytesDone} rounded /> / <Size bytes={bytesTotal} rounded /></span>
        <span class="percent">({formatNumber(bytePercent)}%)</span>
        <span class="rate">
            {#if byteRate !== null}<Trans key="fileOperations.shared.byteRate" snippets={{ size: byteRateSize }} />{/if}
        </span>
    {/if}

    <span class="bar-label">{countLabel}</span>
    <!-- The bar and the percentage wait for a denominator; the cells stay so the
         five-column grid holds its shape and nothing shifts when one lands. -->
    {#if hasCountFraction}
        <ProgressBar
            value={filesDone / filesTotal}
            size={isCompact ? 'sm' : 'md'}
            ariaLabel={tString('fileOperations.transferProgress.fileProgressAria')}
            ariaValuetext={countValueText}
        />
        <span class="amount">{formatNumber(filesDone)} / {formatNumber(filesTotal)}</span>
        <span class="percent">({formatNumber(filePercent)}%)</span>
    {:else}
        <span></span>
        <span class="amount">{formatNumber(filesDone)}</span>
        <span class="percent"></span>
    {/if}
    <span class="rate">{fileRateText ?? ''}</span>

    <!-- Rendered even while empty (see `.time:empty` below): the line holds its
         height so the readout doesn't grow a row, and shove everything under it
         down, the moment the estimator warms up. -->
    <span class="left">{leftText ?? ''}</span>
    <span class="time" class:stalled={stall !== null || waitLine?.kind === 'reconnecting'}
        >{#if receivingRate !== null}<Trans
                key="fileOperations.transferProgress.sourceReceiving"
                snippets={{ size: receivingRateSize }}
            />{:else}{timeText ?? ''}{/if}</span
    >
</div>

{#snippet byteRateSize(children: import('svelte').Snippet)}<Size bytes={byteRate ?? 0} rounded />{@render children()}{/snippet}
{#snippet receivingRateSize(children: import('svelte').Snippet)}<Size
        bytes={receivingRate ?? 0}
        rounded
    />{@render children()}{/snippet}

<style>
    .progress-readout {
        /* Column widths in `ch` (the digit width of the current font), each
           sized for the widest string that column can hold: "999 GB / 999 GB",
           "(100%)", "999 MB/s". `minmax` rather than a hard width so a rarer
           outlier (a byte-scale pair) grows instead of clipping. */
        --spacing-readout-amount: 16ch;
        --spacing-readout-percent: 6.5ch;
        --spacing-readout-rate: 9ch;
        /* Below this the bar reads as a smudge rather than progress, so it's
           the window's min width that gives way, not the bar. */
        --spacing-readout-bar-min: 80px;

        display: grid;
        grid-template-columns:
            auto
            minmax(var(--spacing-readout-bar-min), 1fr)
            minmax(var(--spacing-readout-amount), auto)
            minmax(var(--spacing-readout-percent), auto)
            minmax(var(--spacing-readout-rate), auto);
        align-items: center;
        gap: var(--spacing-xs) var(--spacing-sm);
        font-size: var(--font-size-sm);
        font-variant-numeric: tabular-nums;
    }

    /* A list row keeps the same five columns, tightened: a thinner bar, a lower
       bar floor, and smaller gaps. It keeps the labels too — two unlabelled bars
       stacked in a row read as a puzzle, and the units in the amounts don't
       answer it fast enough. The label column costs the queue window width, and
       `queue-window.ts`'s MIN_WIDTH pays for it. */
    .progress-readout.compact {
        --spacing-readout-bar-min: 64px;

        gap: var(--spacing-xxs) var(--spacing-xs);
    }

    .bar-label {
        color: var(--color-text-tertiary);
    }

    .amount,
    .percent,
    .rate {
        white-space: nowrap;
        text-align: right;
        color: var(--color-text-secondary);
    }

    /* Its own row under the bars: the bars get the width back, and an estimate
       firming up from "1h 8m left" to "56m 24s left" moves nothing else. The
       count left starts the row, the time left ends it. */
    .left {
        grid-column: 1 / 3;
        white-space: nowrap;
        color: var(--color-text-tertiary);
    }

    .time {
        grid-column: 3 / -1;
        justify-self: end;
        white-space: nowrap;
        color: var(--color-text-tertiary);
    }

    /* A no-break space keeps the empty line's box, so the readout's height is
       the same before and after the estimator has something to say. A CSS
       escape, not an invisible character in the markup. */
    .time:empty::before {
        content: '\a0';
    }

    /* A stalled readout is the one thing on the row worth noticing, and so is
       one waiting for its volume to come back. */
    .time.stalled {
        color: var(--color-text-secondary);
    }
</style>
