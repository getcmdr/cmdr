<!--
  Product § Usage: daily active installs (new and returning stacked, update-check reach as a dotted context
  line) and app hours per active install, each a daily chart plus the 90/30/7 stats grid. Today is faded and
  in no stat. WAU and MAU live in the headline tiles, so they get no chart here.
-->
<script lang="ts">
    import type { SourceResult, DashboardSelection } from '#lib/server/types.js'
    import type { ActiveInstallsData } from '#lib/server/sources/active-installs.js'
    import type { CloudflareData } from '#lib/server/sources/cloudflare.js'
    import type { ViewWindow } from '#lib/time-range.js'
    import type { TooltipRow } from '#lib/daily-chart.js'
    import { reachByDay, splitActives } from '#lib/active-installs.js'
    import { cell, countStatsByWindow, dailyRatios, ratioStatsByWindow, rollingComplete, viewRange } from '#lib/trend-row.js'
    import { formatAverage, formatNumber, formatRelativeChange } from '#lib/format.js'
    import { COLOR_GOLD } from '#lib/colors.js'
    import { tint } from '#lib/country-colors.js'
    import DailyChart from '../DailyChart.svelte'
    import StatsGrid from '../StatsGrid.svelte'
    import TrendRow from '../TrendRow.svelte'
    import InfoPopover from '../InfoPopover.svelte'
    import ErrorState from '../ErrorState.svelte'

    interface Props {
        activeInstalls: SourceResult<ActiveInstallsData | null>
        /** Update-check reach, the dotted line. */
        cloudflare: SourceResult<CloudflareData>
        window: ViewWindow
        selection: DashboardSelection
        todayIso: string
    }

    const { activeInstalls, cloudflare, window: shown, selection, todayIso }: Props = $props()

    /** Returning installs: the product's gold, lightened, so the two layers read as one population. */
    const colorReturning = tint(COLOR_GOLD, 0.55)
    /**
     * The reach line: a neutral light grey. Not `COLOR_UNKNOWN` (that grey means "unknown value") and not
     * purple (mtp-rs).
     */
    const colorReach = '#c8c8cf'

    const all = $derived(activeInstalls.ok ? (activeInstalls.data?.days ?? []) : [])
    const dates = $derived(all.map((d) => d.date))
    const range = $derived(viewRange(dates, shown))
    const shownDays = $derived(all.slice(range.start, range.stop))
    const split = $derived(splitActives(shownDays))
    const reach = $derived(cloudflare.ok ? reachByDay(cloudflare.data.updateActivity, shownDays.map((d) => d.date)) : null)

    const dauPoints = $derived(all.map((d) => ({ date: d.date, value: d.dau })))
    const hoursPoints = $derived(all.map((d) => ({ date: d.date, num: d.appHours, den: d.dau })))
    const hoursRolling = $derived(rollingComplete(hoursPoints, todayIso))

    function formatHours(value: number): string {
        return `${formatAverage(value)} h`
    }
    /** Hours pool as a ratio, so the grid's change is their difference, in hours. */
    function formatHoursChange(change: number): string {
        const rounded = Math.round(change * 10) / 10
        return rounded === 0 ? '±0 h' : `${rounded > 0 ? '+' : '−'}${String(Math.abs(rounded))} h`
    }
</script>

<section class="rounded-xl border border-border bg-surface p-4 sm:p-6">
    <h2 class="flex items-center gap-2 text-lg font-semibold text-text-primary">
        Usage
        <InfoPopover topic="usage">
            <p>How many installs run Cmdr each day, and for how long.</p>
            <ul>
                <li>Every day is a UTC day. Today is still going, so it's faded and left out of every stat.</li>
                <li>The stats cover the last 90, 30, and 7 complete days; "vs. previous" compares with the window before.</li>
                <li>Counts installs with usage stats on. An install that opts out sends nothing at all, so these undercount by an unknown amount.</li>
            </ul>
            <p><span class="info-label">Source:</span> the app's heartbeat, via the api-server.</p>
        </InfoPopover>
    </h2>

    {#if activeInstalls.ok}
        {@const unavailable = activeInstalls.data === null ? "Not available yet: the live api-server sends this once its update lands." : null}

        <TrendRow title="Daily active installs" {unavailable}>
            {#snippet info()}
                <p>Distinct installs that sent a heartbeat that day.</p>
                <ul>
                    <li>Gold is new installs (their first heartbeat ever), the lighter gold is everyone returning.</li>
                    <li>
                        The dotted line is the update-check reach: distinct addresses that asked for the latest version. It
                        rides a separate consent, so it also sees installs with analytics off.
                    </li>
                    <li>
                        Read the reach as context, not a ceiling: one office behind a shared address counts once, a
                        changing home address counts twice, and installs with update checks off never show.
                    </li>
                    <li>The last week can still hold a few synthetic installs the weekly sweep removes later.</li>
                </ul>
                <p><span class="info-label">Source:</span> the heartbeat and the update-check log, via the api-server.</p>
            {/snippet}
            {#snippet chart()}
                <DailyChart
                    days={shownDays.map((d) => d.date)}
                    today={todayIso}
                    label="Daily active installs, new and returning"
                    formatValue={formatNumber}
                    series={[
                        { key: 'new', label: 'New', color: COLOR_GOLD, values: split.newInstalls },
                        { key: 'returning', label: 'Returning', color: colorReturning, values: split.returning },
                    ]}
                    overlay={reach ? { label: 'Update-check reach', color: colorReach, values: reach } : undefined}
                    tooltip={(i: number): TooltipRow[] => [
                        { label: 'Active installs', value: cell(shownDays[i].dau) },
                        { label: 'New', value: cell(split.newInstalls[i]), color: COLOR_GOLD },
                        { label: 'Returning', value: cell(split.returning[i]), color: colorReturning },
                    ]}
                />
                {#if !reach}
                    <p class="mt-1 pl-11 text-xs text-text-tertiary">The update-check reach couldn't load.</p>
                {/if}
            {/snippet}
            {#snippet stats()}
                <StatsGrid stats={countStatsByWindow(dauPoints, todayIso)} formatValue={formatAverage} formatChange={formatRelativeChange} />
            {/snippet}
        </TrendRow>

        <TrendRow title="Hours per active install" {unavailable}>
            {#snippet info()}
                <p>How long Cmdr ran on a day, per install that ran it.</p>
                <ul>
                    <li>Dots are each day's app hours ÷ that day's active installs.</li>
                    <li>The line is the 7-day rate, pooled (the week's hours ÷ the week's actives).</li>
                    <li>A beat from a build too old to report its runtime counts as an hour.</li>
                </ul>
                <p><span class="info-label">Source:</span> the app's heartbeat, via the api-server.</p>
            {/snippet}
            {#snippet chart()}
                <DailyChart
                    days={shownDays.map((d) => d.date)}
                    today={todayIso}
                    label="Daily app hours per active install"
                    formatValue={formatHours}
                    rate={{
                        color: COLOR_GOLD,
                        daily: dailyRatios(hoursPoints).slice(range.start, range.stop),
                        rolling: hoursRolling.slice(range.start, range.stop),
                        rollingLabel: '7-day rate',
                    }}
                    tooltip={(i: number): TooltipRow[] => {
                        const d = shownDays[i]
                        return [
                            { label: 'Hours per install', value: cell(d.dau > 0 ? d.appHours / d.dau : null, formatHours), color: COLOR_GOLD, mark: 'dot' },
                            { label: 'App hours / installs', value: `${formatAverage(d.appHours)} / ${formatNumber(d.dau)}` },
                            { label: '7-day rate', value: cell(hoursRolling[range.start + i], formatHours), color: COLOR_GOLD, mark: 'line' },
                        ]
                    }}
                />
            {/snippet}
            {#snippet stats()}
                <StatsGrid stats={ratioStatsByWindow(hoursPoints, todayIso)} formatValue={formatHours} formatChange={formatHoursChange} />
            {/snippet}
        </TrendRow>
    {:else}
        <ErrorState error={activeInstalls.error} {selection} />
    {/if}
</section>
