<!--
  Product § headline: six tiles read on yesterday (today is partial), each against the picker's N days.
  Every number comes from `#lib/product-stats.ts`; this only picks inputs and formats. A source that failed
  reads "Couldn't load", an older Worker without the data reads "Not available yet".
-->
<script lang="ts">
    import type { SourceResult } from '#lib/server/types.js'
    import type { ActiveInstallsData } from '#lib/server/sources/active-installs.js'
    import type { FunnelData } from '#lib/server/sources/funnel.js'
    import type { PaddleData } from '#lib/server/sources/paddle.js'
    import {
        d7Tile,
        hoursPerActiveDayTile,
        mauTile,
        payingShareTile,
        stickinessTile,
        wauTile,
        type Tile,
    } from '#lib/product-stats.js'
    import { shiftDay } from '#lib/time-range.js'
    import type { ChangeFormat } from '#lib/format.js'
    import { changeTone, formatAverage, formatNumber, formatPercent, formatPointChange, formatRelativeChange } from '#lib/format.js'
    import StatTile from '../StatTile.svelte'

    interface Props {
        activeInstalls: SourceResult<ActiveInstallsData | null>
        funnel: SourceResult<FunnelData>
        paddle: SourceResult<PaddleData>
        /** The picker's range in days: every tile's comparison. */
        n: number
        todayIso: string
    }

    const { activeInstalls, funnel, paddle, n, todayIso }: Props = $props()

    const couldntLoad = "Couldn't load"
    const notYet = 'Not available yet'

    const days = $derived(activeInstalls.ok ? (activeInstalls.data?.days ?? null) : null)
    /** Why the heartbeat tiles can't draw, or null when they can. */
    const activesReason = $derived(!activeInstalls.ok ? couldntLoad : activeInstalls.data === null ? notYet : null)
    const funnelRows = $derived(funnel.ok && funnel.data.workerStatus !== 'unavailable' ? funnel.data.rows : null)

    const tiles = $derived({
        wau: days ? wauTile(days, n, todayIso) : null,
        mau: days ? mauTile(days, n, todayIso) : null,
        stickiness: days ? stickinessTile(days, n, todayIso) : null,
        hours: days ? hoursPerActiveDayTile(days, n, todayIso) : null,
        d7: d7Tile(funnelRows, n, todayIso),
        paying: days && paddle.ok ? payingShareTile(paddle.data.allTransactions, days, n, todayIso) : null,
    })

    function show(tile: Tile | null, format: (v: number) => string, formatChange: ChangeFormat) {
        return {
            value: tile?.value == null ? null : format(tile.value),
            change: tile?.change == null ? null : formatChange(tile.change),
            tone: changeTone(tile?.change ?? null, formatChange),
        }
    }

    const earlier = $derived(`vs. ${String(n)} days earlier`)
    const previous = $derived(`vs. previous ${String(n)} days`)
</script>

<section class="rounded-xl border border-border bg-surface p-4 sm:p-6">
    <p class="mb-3 text-xs text-text-tertiary">Yesterday, {shiftDay(todayIso, -1)} (UTC), the last complete day</p>
    <div class="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
        <StatTile label="Weekly active" {...show(tiles.wau, formatNumber, formatRelativeChange)} comparison={earlier} unavailable={activesReason}>
            {#snippet info()}
                <p>Installs that sent a heartbeat in the seven days ending yesterday.</p>
                <ul>
                    <li>Compared with the same count {n} days earlier.</li>
                    <li>Counts installs with usage stats on; an install that opted out sends nothing.</li>
                    <li>The last week can still hold a few synthetic installs the weekly sweep removes later.</li>
                </ul>
                <p><span class="info-label">Source:</span> the app's heartbeat, via the api-server.</p>
            {/snippet}
        </StatTile>
        <StatTile label="Monthly active" {...show(tiles.mau, formatNumber, formatRelativeChange)} comparison={earlier} unavailable={activesReason}>
            {#snippet info()}
                <p>Installs that sent a heartbeat in the 28 days ending yesterday.</p>
                <ul>
                    <li>28 days is four whole weeks, so the weekday mix never shifts it.</li>
                    <li>Compared with the same count {n} days earlier.</li>
                </ul>
                <p><span class="info-label">Source:</span> the app's heartbeat, via the api-server.</p>
            {/snippet}
        </StatTile>
        <StatTile label="Stickiness" {...show(tiles.stickiness, formatPercent, formatPointChange)} comparison={previous} unavailable={activesReason}>
            {#snippet info()}
                <p>How many of the month's installs show up on an average day.</p>
                <ul>
                    <li>Daily active ÷ monthly active, pooled over the last {n} complete days (Σ DAU ÷ Σ MAU).</li>
                    <li>Compared with the {n} days before, in percentage points.</li>
                </ul>
                <p><span class="info-label">Source:</span> the app's heartbeat, via the api-server.</p>
            {/snippet}
        </StatTile>
        <StatTile
            label="Hours per active day"
            {...show(tiles.hours, (v) => `${formatAverage(v)} h`, formatRelativeChange)}
            comparison={previous}
            unavailable={activesReason}
        >
            {#snippet info()}
                <p>How long Cmdr runs on a day an install uses it.</p>
                <ul>
                    <li>All app hours ÷ all daily actives over the last {n} complete days.</li>
                    <li>A beat from a build too old to report its runtime counts as an hour.</li>
                </ul>
                <p><span class="info-label">Source:</span> the app's heartbeat, via the api-server.</p>
            {/snippet}
        </StatTile>
        <StatTile
            label="D7 retention"
            {...show(tiles.d7, formatPercent, formatPointChange)}
            comparison="vs. previous {n} cohorts"
            unavailable={funnelRows === null ? couldntLoad : null}
        >
            {#snippet info()}
                <p>The share of new installs still sending heartbeats on their 7th day.</p>
                <ul>
                    <li>Pooled over the {n} newest cohorts old enough to have a D7 (eight days and up).</li>
                    <li>Compared with the {n} cohorts before, in percentage points.</li>
                </ul>
                <p><span class="info-label">Source:</span> the api-server's funnel.</p>
            {/snippet}
        </StatTile>
        <StatTile
            label="Paying share"
            {...show(tiles.paying, formatPercent, formatPointChange)}
            comparison={earlier}
            unavailable={!paddle.ok ? couldntLoad : activesReason}
        >
            {#snippet info()}
                <p>Paid licenses ÷ monthly active installs, on yesterday.</p>
                <ul>
                    <li>Paid licenses are every first purchase through yesterday; renewals and card updates don't count.</li>
                    <li>A trend signal, not a conversion rate: refunded purchases still count, one license can cover several Macs, and a licensed install with analytics off is paid but not in monthly active.</li>
                    <li>Compared with {n} days earlier, in percentage points.</li>
                </ul>
                <p><span class="info-label">Source:</span> Paddle and the app's heartbeat.</p>
            {/snippet}
        </StatTile>
    </div>
</section>
