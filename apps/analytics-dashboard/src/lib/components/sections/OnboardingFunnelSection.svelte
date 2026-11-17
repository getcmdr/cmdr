<!--
  Product § Onboarding: how far installs get, from the state flags in settings adoption (each install's
  latest heartbeat). One step bar (finished onboarding), then share bars for the hints people discovered,
  the one that means they hit a snag, and the ones they closed by hand. Each share is over the installs
  whose build has that flag; below `minEligible` a row prints "n of m" with no bar. The flag list and its
  meaning live in `#lib/onboarding-funnel.ts`.
-->
<script lang="ts">
    import type { SourceResult, DashboardSelection } from '#lib/server/types.js'
    import type { SettingsAdoption } from '#lib/server/settings-defaults.js'
    import type { FlagReach } from '#lib/onboarding-funnel.js'
    import { minEligible, onboardingFunnel } from '#lib/onboarding-funnel.js'
    import { formatNumber, formatPercent } from '#lib/format.js'
    import { COLOR_GOLD } from '#lib/colors.js'
    import InfoPopover from '../InfoPopover.svelte'
    import ErrorState from '../ErrorState.svelte'

    const {
        settingsAdoption,
        selection,
    }: {
        settingsAdoption: SourceResult<SettingsAdoption>
        selection: DashboardSelection
    } = $props()
</script>

{#snippet row(reach: FlagReach)}
    <tr class="border-b border-border-subtle/50" title={reach.key}>
        <td class="py-1.5 pr-4 text-text-primary">
            {reach.label}{#if reach.capabilityGated}<span class="text-text-tertiary" aria-label="(needs an OS feature)">*</span>{/if}
        </td>
        <td class="py-1.5 pr-4">
            {#if reach.share !== null}
                <span class="flex items-center gap-2">
                    <span class="flex h-1.5 w-12 shrink-0 overflow-hidden rounded-full bg-border sm:w-40">
                        <span class="h-full rounded-full" style="width: {reach.share * 100}%; background-color: {COLOR_GOLD};"></span>
                    </span>
                    <span class="w-10 text-right text-text-primary tabular-nums">{formatPercent(reach.share)}</span>
                </span>
            {:else}
                <span class="text-text-tertiary">Too few to say</span>
            {/if}
        </td>
        <td class="py-1.5 text-right whitespace-nowrap text-text-secondary tabular-nums">
            {formatNumber(reach.reached)} of {formatNumber(reach.eligible)}
        </td>
    </tr>
{/snippet}

{#snippet group(title: string, reaches: FlagReach[])}
    {#if reaches.length > 0}
        <tbody>
            <tr>
                <th colspan="3" class="pt-4 pb-1 text-left text-xs font-medium text-text-tertiary">{title}</th>
            </tr>
            {#each reaches as reach (reach.key)}
                {@render row(reach)}
            {/each}
        </tbody>
    {/if}
{/snippet}

<section class="rounded-xl border border-border bg-surface p-4 sm:p-6">
    <h2 class="mb-1 flex items-center gap-2 text-lg font-semibold text-text-primary">
        Onboarding
        <InfoPopover topic="onboarding">
            <p>How far installs get through onboarding and the once-only hints.</p>
            <ul>
                <li>Each install counts once, at its latest heartbeat. A flag off its default means the install saw, did, or closed that thing.</li>
                <li>Each share is over the installs whose build has that flag. An older build without it is left out, never counted as a no.</li>
                <li>Under {minEligible} such installs, a row shows the count only, since a share would be noise.</li>
                <li>
                    A * marks a hint that only shows where macOS has the feature behind it, so its share also counts installs
                    that could never see it.
                </li>
                <li>"Hit a snag" is a hint that only shows when something didn't go the way the person expected.</li>
            </ul>
            <p><span class="info-label">Source:</span> settings in the app's heartbeat, via the api-server.</p>
        </InfoPopover>
    </h2>

    {#if settingsAdoption.ok}
        {@const funnel = onboardingFunnel(settingsAdoption.data.settings)}
        {#if funnel.step === null && funnel.discovered.length + funnel.friction.length + funnel.dismissed.length === 0}
            <p class="text-sm text-text-tertiary">No install runs a build with these flags yet.</p>
        {:else}
            <div class="overflow-x-auto">
                <table class="w-full text-left text-sm">
                    <thead class="sr-only">
                        <tr>
                            <th>Step</th>
                            <th>Share</th>
                            <th>Installs</th>
                        </tr>
                    </thead>
                    {#if funnel.step}
                        {@render group('Step', [funnel.step])}
                    {/if}
                    {@render group('Discovered', funnel.discovered)}
                    {@render group('Hit a snag', funnel.friction)}
                    {@render group('Closed by hand', funnel.dismissed)}
                </table>
            </div>
        {/if}
    {:else}
        <ErrorState error={settingsAdoption.error} {selection} />
    {/if}
</section>
