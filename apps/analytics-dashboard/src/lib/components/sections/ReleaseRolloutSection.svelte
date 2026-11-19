<!--
  Product § Release rollout: how far the latest release has spread (yesterday's share of active installs),
  over a daily stack of active installs by the version each ran. Every version in view draws its own layer,
  oldest at the bottom, with no "Older" bucket and no folding of thin layers: the eight newest wear strong
  colors and older ones a fading ramp (`versionColors`). The legend and tooltip list newest first, older
  versions' legend labels muted. Today is faded and not in the headline.
-->
<script lang="ts">
    import type { SourceResult, DashboardSelection } from '#lib/server/types.js'
    import type { ActiveInstallsData } from '#lib/server/sources/active-installs.js'
    import type { LatestRelease } from '#lib/rollout.js'
    import type { ViewWindow } from '#lib/time-range.js'
    import type { TooltipRow } from '#lib/daily-chart.js'
    import { breakdownRows } from '#lib/daily-chart.js'
    import { rolloutHeadline, rolloutSentence, versionStack } from '#lib/rollout.js'
    import { latestCount, versionColors } from '#lib/version-colors.js'
    import { cell, viewRange } from '#lib/trend-row.js'
    import { formatNumber } from '#lib/format.js'
    import DailyChart from '../DailyChart.svelte'
    import InfoPopover from '../InfoPopover.svelte'
    import ErrorState from '../ErrorState.svelte'

    interface Props {
        activeInstalls: SourceResult<ActiveInstallsData | null>
        /** The release the headline tracks, or null when neither GitHub nor the heartbeat names one. */
        release: LatestRelease | null
        window: ViewWindow
        selection: DashboardSelection
        todayIso: string
    }

    const { activeInstalls, release, window: shown, selection, todayIso }: Props = $props()

    const all = $derived(activeInstalls.ok ? (activeInstalls.data?.days ?? []) : [])
    const range = $derived(viewRange(all.map((d) => d.date), shown))
    const shownDays = $derived(all.slice(range.start, range.stop))
    const stack = $derived(versionStack(shownDays))
    const newestFirst = $derived([...stack.versions].reverse())
    const colors = $derived(versionColors(stack.versions))
    const colorOf = (version: string): string => colors.get(version) ?? ''
</script>

<section class="rounded-xl border border-border bg-surface p-4 sm:p-6">
    <h2 class="flex items-center gap-2 text-lg font-semibold text-text-primary">
        Release rollout
        <InfoPopover topic="release rollout">
            <p>Which version each active install ran, day by day, and how far the latest release has spread.</p>
            <ul>
                <li>Each install counts once a day, under the highest version it ran that day, so a day's layers add up to its active installs.</li>
                <li>The headline is yesterday's share on the newest stable GitHub release; today is still going, so it's faded and left out.</li>
                <li>The eight newest versions get strong colors; older ones fade with age. No version folds into an "Older" bucket.</li>
                <li>Counts installs with usage stats on, like the usage charts above.</li>
            </ul>
            <p><span class="info-label">Source:</span> the app's heartbeat via the api-server, and GitHub releases.</p>
        </InfoPopover>
    </h2>

    {#if activeInstalls.ok}
        {#if activeInstalls.data === null}
            <p class="mt-3 text-sm text-text-tertiary">Not available yet: the live api-server sends this once its update lands.</p>
        {:else}
            {#if release}
                <p class="mt-2 mb-4 text-base text-text-primary">{rolloutSentence(rolloutHeadline(release, all, todayIso))}</p>
            {/if}
            <DailyChart
                days={stack.days}
                today={todayIso}
                label="Daily active installs by app version"
                formatValue={formatNumber}
                height={220}
                series={stack.versions.map((version) => ({
                    key: version,
                    label: `v${version}`,
                    color: colorOf(version),
                    values: stack.series[version],
                }))}
                legend={newestFirst.map((version, rank) => ({
                    label: `v${version}`,
                    color: colorOf(version),
                    muted: rank >= latestCount,
                }))}
                tooltip={(i: number): TooltipRow[] => [
                    { label: 'Active installs', value: cell(shownDays[i].dau) },
                    ...breakdownRows(
                        newestFirst.map((version) => ({
                            label: `v${version}`,
                            color: colorOf(version),
                            count: stack.series[version][i],
                        })),
                        shownDays[i].dau,
                        { sort: false },
                    ),
                ]}
            />
        {/if}
    {:else}
        <ErrorState error={activeInstalls.error} {selection} />
    {/if}
</section>
