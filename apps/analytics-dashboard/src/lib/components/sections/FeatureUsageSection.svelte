<!--
  Product § Feature usage: per feature (the `feature-status.json` ids), the share of the window's active
  installs that used it, as a bar, with its change against the window before in percentage points. The
  window is the picker's complete days ending yesterday; an older Worker only answers a rolling window that
  includes today, and the window line says so. The mapping lives in the api-server
  (`featureUsageDefinitions`); this only renders what comes back.
-->
<script lang="ts">
    import type { SourceResult, DashboardSelection } from '#lib/server/types.js'
    import type { FeatureUsagePeriods } from '#lib/feature-usage.js'
    import { featureShare, foldNoData, formatParts, shareBarWidth, shareChange, sortFeatureUsage } from '#lib/feature-usage.js'
    import { changeTone, changeToneClass, formatNumber, formatPercent, formatPointChange } from '#lib/format.js'
    import { shiftDay } from '#lib/time-range.js'
    import { COLOR_GOLD } from '#lib/colors.js'
    import InfoPopover from '../InfoPopover.svelte'
    import ErrorState from '../ErrorState.svelte'

    const {
        featureUsage,
        selection,
    }: {
        featureUsage: SourceResult<FeatureUsagePeriods>
        selection: DashboardSelection
    } = $props()

    function changeClass(change: number | null): string {
        return changeToneClass[changeTone(change, formatPointChange)]
    }
</script>

<section class="rounded-xl border border-border bg-surface p-4 sm:p-6">
    <h2 class="mb-1 flex items-center gap-2 text-lg font-semibold text-text-primary">
        Feature usage
        <InfoPopover topic="feature usage">
            <p>The share of active installs that used each feature in the window.</p>
            <ul>
                <li>An install counts toward a feature once it fired any of the feature's events. Hover a row for which events those are.</li>
                <li>The share divides by installs that sent at least one heartbeat in the window.</li>
                <li>
                    "vs. previous" compares with the same-length window before, in percentage points. It's a dash when the
                    feature's events weren't stored yet back then, so a new event can't read as growth from nothing.
                </li>
                <li>"No data yet" means no release has sent that event yet, which is different from nobody using it.</li>
            </ul>
            <p><span class="info-label">Source:</span> the app's usage events, via the api-server.</p>
        </InfoPopover>
    </h2>

    {#if featureUsage.ok}
        {@const periods = featureUsage.data}
        {@const current = periods.current}
        {@const { seen, noData } = foldNoData(sortFeatureUsage(current.features))}
        <p class="mb-3 text-sm text-text-secondary">
            {formatNumber(current.activeInstalls)} active {current.activeInstalls === 1 ? 'install' : 'installs'},
            {#if current.until}
                {current.since.slice(0, 10)} – {shiftDay(current.until.slice(0, 10), -1)}
            {:else}
                since {current.since.slice(0, 10)}, up to now (today's partial hours included)
            {/if}
        </p>
        <div class="overflow-x-auto">
            <table class="w-full text-left text-sm">
                <thead>
                    <tr class="border-b border-border-subtle text-xs text-text-tertiary">
                        <th class="pr-4 pb-2 font-medium">Feature</th>
                        <th class="pr-4 pb-2 font-medium">Share</th>
                        <th class="pr-4 pb-2 text-right font-medium">vs. previous</th>
                        <th class="pr-4 pb-2 text-right font-medium">Installs</th>
                        <th class="pb-2 text-right font-medium">Events</th>
                    </tr>
                </thead>
                <tbody>
                    {#each seen as feature (feature.id)}
                        {@const share = featureShare(feature, current.activeInstalls)}
                        {@const change = shareChange(feature, periods)}
                        <tr class="border-b border-border-subtle/50" title={feature.sources.join(', ')}>
                            <td class="py-1.5 pr-4 text-text-primary">
                                {feature.name}
                                {#if feature.parts.length > 0}
                                    <span class="block text-xs text-text-tertiary">{formatParts(feature.parts)}</span>
                                {/if}
                            </td>
                            <td class="py-1.5 pr-4">
                                <span class="flex items-center gap-2">
                                    <span class="flex h-1.5 w-12 shrink-0 overflow-hidden rounded-full bg-border sm:w-28">
                                        <span
                                            class="h-full rounded-full"
                                            style="width: {shareBarWidth(feature, current.activeInstalls) * 100}%; background-color: {COLOR_GOLD};"
                                        ></span>
                                    </span>
                                    <span class="w-10 text-right text-text-primary tabular-nums">{share === null ? '–' : formatPercent(share)}</span>
                                </span>
                            </td>
                            <td class="py-1.5 pr-4 text-right tabular-nums {changeClass(change)}">
                                {change === null ? '–' : formatPointChange(change)}
                            </td>
                            <td class="py-1.5 pr-4 text-right text-text-secondary tabular-nums">{formatNumber(feature.installs)}</td>
                            <td class="py-1.5 text-right text-text-secondary tabular-nums">{formatNumber(feature.events)}</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
        {#if noData.length > 0}
            <p class="mt-3 text-xs text-text-tertiary">No data yet: {noData.map((f) => f.name).join(', ')}</p>
        {/if}
    {:else}
        <ErrorState error={featureUsage.error} {selection} />
    {/if}
</section>
