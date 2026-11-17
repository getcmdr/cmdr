<!--
  Product § Retention: D7 and D28 by install-day cohort, all countries, each a daily chart plus the
  90/30/7 stats grid. Young cohorts show grey "so far" points; every stat pools final cohorts only
  (`#lib/acquisition-series.ts` decides which are final).
-->
<script lang="ts">
    import type { SourceResult, DashboardSelection } from '#lib/server/types.js'
    import type { FunnelData, FunnelRow } from '#lib/server/sources/funnel.js'
    import type { ViewWindow } from '#lib/time-range.js'
    import { rollingDays } from '#lib/time-range.js'
    import type { RatioPoint } from '#lib/acquisition-stats.js'
    import { rollingRatio } from '#lib/acquisition-stats.js'
    import { buildTrendInputs, d7End, d28End, rateBands, type TentativePoint } from '#lib/acquisition-series.js'
    import { bandsLabel } from '#lib/acquisition-by-country.js'
    import { safeRatio } from '#lib/acquisition-countries.js'
    import type { TooltipRow } from '#lib/daily-chart.js'
    import { cell, dailyRatios, ratioStatsByWindow, viewRange } from '#lib/trend-row.js'
    import { formatPercent, formatPointChange } from '#lib/format.js'
    import { COLOR_GOLD } from '#lib/colors.js'
    import DailyChart from '../DailyChart.svelte'
    import StatsGrid from '../StatsGrid.svelte'
    import TrendRow from '../TrendRow.svelte'
    import InfoPopover from '../InfoPopover.svelte'
    import ErrorState from '../ErrorState.svelte'

    interface Props {
        funnel: SourceResult<FunnelData>
        /** The days the charts show: the range plus today. */
        window: ViewWindow
        selection: DashboardSelection
        todayIso: string
    }

    const { funnel, window: shown, selection, todayIso }: Props = $props()
    const tentativeColor = '#8a8a93'

    const rows = $derived(funnel.ok ? funnel.data.rows : [])
    const inputs = $derived(buildTrendInputs(rows, todayIso))
    const range = $derived(viewRange(inputs.days, shown))
    const days = $derived(inputs.days.slice(range.start, range.stop))
    // Non-final cohorts carry no numerator, so the lines only pool final cohorts.
    const rolling = $derived({ d7: rollingRatio(inputs.d7, rollingDays), d28: rollingRatio(inputs.d28, rollingDays) })

    function view<T>(values: T[]): T[] {
        return values.slice(range.start, range.stop)
    }

    /** A chart day's readout: a final cohort's rate and counts, or a young one's "so far" point. */
    function retentionTooltip(
        name: string,
        points: RatioPoint[],
        tentative: (TentativePoint | null)[],
        line: (number | null)[],
        soFar: (r: FunnelRow) => { retained: number } | null,
        i: number,
    ): TooltipRow[] {
        const at = range.start + i
        const r = rows[at]
        const p = points[at]
        const t = tentative[at]
        if (p.num === null) {
            return [
                {
                    label: t ? t.label : name,
                    value: t ? `${formatPercent(t.value)} (${cell(soFar(r)?.retained ?? null)} of ${cell(r.newInstalls)})` : 'too young',
                    color: tentativeColor,
                    mark: 'dot',
                },
            ]
        }
        return [
            { label: name, value: cell(safeRatio(p.num, p.den), formatPercent), color: COLOR_GOLD, mark: 'dot' },
            { label: 'Retained / cohort', value: `${cell(p.num)} / ${cell(p.den)}` },
            { label: '7-cohort rate', value: cell(line[at], formatPercent), color: COLOR_GOLD, mark: 'line' },
        ]
    }
</script>

<section class="rounded-xl border border-border bg-surface p-4 sm:p-6">
    <h2 class="flex items-center gap-2 text-lg font-semibold text-text-primary">
        Retention
        <InfoPopover topic="retention">
            <p>Do new installs come back? One row per cohort age, all countries.</p>
            <ul>
                <li>A cohort is the installs whose first heartbeat landed that day.</li>
                <li>The stats count back from the newest final cohort, so "7d" is the seven newest cohorts that have the number.</li>
                <li>Rates pool as all retained ÷ all installs, so a one-install day can't swing them.</li>
            </ul>
            <p><span class="info-label">Source:</span> the api-server, which owns the definition.</p>
        </InfoPopover>
    </h2>

    {#if funnel.ok}
        {@const workerReason = funnel.data.workerStatus === 'unavailable' ? "Couldn't load this from the api-server." : null}

        <TrendRow title="D7 retention" unavailable={workerReason}>
            {#snippet info()}
                <p>The share of a day's new installs that sent any heartbeat on their 7th day after install.</p>
                <ul>
                    <li>
                        A cohort needs to be at least eight days old to have a D7, so the newest week shows grey "so far"
                        points instead: retained as of day N, which can only grow.
                    </li>
                    <li>The line pools the last seven final cohorts.</li>
                    <li>Targets: under 15% is weak, 25% and up is strong.</li>
                </ul>
                <p><span class="info-label">Source:</span> the api-server.</p>
            {/snippet}
            {#snippet chart()}
                <DailyChart
                    {days}
                    today={todayIso}
                    label="D7 retention by install day"
                    formatValue={formatPercent}
                    rate={{
                        color: COLOR_GOLD,
                        daily: view(dailyRatios(inputs.d7)),
                        rolling: view(rolling.d7),
                        rollingLabel: '7-cohort rate',
                        tentative: inputs.has.retentionSoFar ? view(inputs.d7Tentative) : undefined,
                        bands: rateBands.d7,
                        bandsLabel: bandsLabel(rateBands.d7, formatPercent),
                    }}
                    tooltip={(i: number): TooltipRow[] => retentionTooltip('D7', inputs.d7, inputs.d7Tentative, rolling.d7, (r) => r.retentionSoFar, i)}
                />
                {#if !inputs.has.retentionSoFar}
                    <p class="mt-1 pl-11 text-xs text-text-tertiary">"So far" points for young cohorts aren't available yet.</p>
                {/if}
            {/snippet}
            {#snippet stats()}
                <StatsGrid stats={ratioStatsByWindow(inputs.d7, d7End(todayIso))} formatValue={formatPercent} formatChange={formatPointChange} />
            {/snippet}
        </TrendRow>

        <TrendRow
            title="D28 retention"
            unavailable={workerReason ?? (inputs.has.d28 ? null : 'Not available yet: the live api-server sends D28 once its update lands.')}
        >
            {#snippet info()}
                <p>The share of a day's new installs that sent any heartbeat on their 28th day after install.</p>
                <ul>
                    <li>
                        A cohort needs to be at least 29 days old to have a D28, so cohorts 8–28 days old show grey "so
                        far" points: retained as of day N, which can only grow.
                    </li>
                    <li>The line pools the last seven final cohorts.</li>
                    <li>Targets: under 10% is weak, 20% and up is strong.</li>
                </ul>
                <p><span class="info-label">Source:</span> the api-server.</p>
            {/snippet}
            {#snippet chart()}
                <DailyChart
                    {days}
                    today={todayIso}
                    label="D28 retention by install day"
                    formatValue={formatPercent}
                    rate={{
                        color: COLOR_GOLD,
                        daily: view(dailyRatios(inputs.d28)),
                        rolling: view(rolling.d28),
                        rollingLabel: '7-cohort rate',
                        tentative: inputs.has.d28SoFar ? view(inputs.d28Tentative) : undefined,
                        bands: rateBands.d28,
                        bandsLabel: bandsLabel(rateBands.d28, formatPercent),
                    }}
                    tooltip={(i: number): TooltipRow[] => retentionTooltip('D28', inputs.d28, inputs.d28Tentative, rolling.d28, (r) => r.d28SoFar, i)}
                />
            {/snippet}
            {#snippet stats()}
                <StatsGrid stats={ratioStatsByWindow(inputs.d28, d28End(todayIso))} formatValue={formatPercent} formatChange={formatPointChange} />
            {/snippet}
        </TrendRow>
    {:else}
        <ErrorState error={funnel.error} {selection} />
    {/if}
</section>
