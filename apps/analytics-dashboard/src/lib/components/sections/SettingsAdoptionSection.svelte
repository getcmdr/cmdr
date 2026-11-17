<!--
  Product § Settings adoption: what installs actually run, once an absent config key has been resolved
  against the defaults that shipped in that install's app version. Preferences only (remembered UI and
  onboarding state live in the Onboarding section), the 15 most-moved first, the rest behind "Show all".
-->
<script lang="ts">
    import type { SourceResult, DashboardSelection } from '#lib/server/types.js'
    import type { SettingAdoption, SettingsAdoption } from '#lib/server/settings-defaults.js'
    import { formatNumber } from '#lib/format.js'
    import { COLOR_GOLD, COLOR_GREEN, COLOR_CYAN } from '#lib/colors.js'
    import {
        formatShare,
        formatShareUnlike,
        formatValueShare,
        mostChanged,
        preferencesOnly,
        settingByKey,
        shareOnDefault,
        topMoved,
        topOverride,
        unchangedNote,
    } from '#lib/settings-adoption.js'
    import ErrorState from '#lib/components/ErrorState.svelte'
    import MetricRow from '#lib/components/MetricRow.svelte'
    import InfoPopover from '#lib/components/InfoPopover.svelte'
    import BetaEmptyState from '#lib/components/BetaEmptyState.svelte'

    const {
        settingsAdoption,
        selection,
    }: {
        settingsAdoption: SourceResult<SettingsAdoption>
        selection: DashboardSelection
    } = $props()

    /** The table leads with this many; the rest wait behind "Show all". */
    const topCount = 15
</script>

{#snippet settingsTable(settings: SettingAdoption[])}
    <div class="overflow-x-auto">
        <table class="w-full text-left text-sm">
            <thead>
                <tr class="border-b border-border-subtle text-text-tertiary">
                    <th class="pr-4 pb-2 font-medium">Setting</th>
                    <th class="pr-4 pb-2 font-medium">Default</th>
                    <th class="pr-4 pb-2 text-right font-medium">Installs</th>
                    <th class="pr-4 pb-2 text-right font-medium">On default</th>
                    <th class="pb-2 font-medium">Most common change</th>
                </tr>
            </thead>
            <tbody>
                {#each settings as setting (setting.key)}
                    {@const share = shareOnDefault(setting)}
                    {@const override = topOverride(setting)}
                    <tr class="border-b border-border-subtle/50">
                        <td class="py-1.5 pr-4 font-mono text-xs text-text-primary">{setting.key}</td>
                        <td class="py-1.5 pr-4 text-text-secondary">
                            {setting.defaultLabel ?? setting.latestDefaultLabel}
                            {#if setting.defaultLabel === null && setting.defaultMovedIn}
                                <span class="block text-xs text-text-tertiary">moved in v{setting.defaultMovedIn}</span>
                            {/if}
                        </td>
                        <td class="py-1.5 pr-4 text-right text-text-secondary tabular-nums">
                            {formatNumber(setting.eligible)}
                        </td>
                        <td class="py-1.5 pr-4 text-right text-text-secondary tabular-nums">
                            {share === null ? '–' : formatShare(share)}
                        </td>
                        <td class="py-1.5 text-text-secondary">
                            {#if override}
                                {override.label}
                                <span class="text-text-tertiary">({formatNumber(override.installs)})</span>
                            {:else}
                                –
                            {/if}
                        </td>
                    </tr>
                {/each}
            </tbody>
        </table>
    </div>
{/snippet}

<section class="rounded-xl border border-border bg-surface p-4 sm:p-6">
    <div class="mb-3">
        <h2 class="flex items-center gap-2 text-lg font-semibold text-text-primary">
            Settings adoption
            <InfoPopover topic="settings adoption">
                <p>What installs actually run, setting by setting, defaults included.</p>
                <ul>
                    <li>
                        The app saves only settings someone changed, so a missing one means "on the default" or "this
                        build didn't have it". Each is resolved against the defaults that shipped in that install's version.
                    </li>
                    <li>Each install counts once, at its latest heartbeat.</li>
                    <li>
                        A build older than a setting is left out of that setting's installs, so a young setting has a smaller
                        total than an old one. That's the honest denominator.
                    </li>
                    <li>Only preferences show here. Remembered UI and onboarding state live in the Onboarding section.</li>
                    <li>A value someone set back to the default counts as the default: what matters is what the app runs with.</li>
                    <li>"Moved in" names the version whose default the newest builds ship, when the window holds builds with an older default.</li>
                </ul>
                <p><span class="info-label">Source:</span> settings in the app's heartbeat, via the api-server.</p>
            </InfoPopover>
        </h2>
        <p class="text-sm text-text-tertiary">What do people actually turn on?</p>
    </div>

    {#if settingsAdoption.ok}
        {@const data = settingsAdoption.data}
        {@const readable = data.totalInstalls - data.unresolvedInstalls}
        {@const indexing = settingByKey(data.settings, 'indexing.enabled')}
        {@const mediaIndex = settingByKey(data.settings, 'mediaIndex.enabled')}
        {@const ai = settingByKey(data.settings, 'ai.provider')}
        {@const preferences = preferencesOnly(data.settings)}
        {@const changed = mostChanged(preferences)}

        {#if readable > 0}
            <MetricRow
                metrics={[
                    { label: 'Installs we can read', value: formatNumber(readable), color: COLOR_GOLD },
                    { label: 'Drive indexing on', value: formatValueShare(indexing, 'on'), color: COLOR_GREEN },
                    { label: 'Image search on', value: formatValueShare(mediaIndex, 'on') },
                    { label: 'AI switched on', value: formatShareUnlike(ai, 'off'), color: COLOR_CYAN },
                ]}
            />

            {#if data.unresolvedInstalls > 0}
                <p class="mt-3 text-xs text-text-tertiary">
                    {formatNumber(data.unresolvedInstalls)} more
                    {data.unresolvedInstalls === 1 ? 'install runs' : 'installs run'} a version older than the settings
                    history goes back, so nothing here counts them.
                </p>
            {/if}

            <div class="mt-6 border-t border-border-subtle pt-5">
                <h3 class="mb-2 text-sm font-medium text-text-secondary">Settings people change</h3>
                {#if changed.length > 0}
                    {@render settingsTable(topMoved(preferences, topCount))}
                    {#if changed.length > topCount}
                        <details class="mt-3">
                            <summary class="cursor-pointer text-sm text-text-secondary hover:text-text-primary">
                                Show all ({formatNumber(changed.length)})
                            </summary>
                            <div class="mt-2">{@render settingsTable(changed.slice(topCount))}</div>
                        </details>
                    {/if}
                {:else}
                    <p class="text-sm text-text-tertiary">Nobody has moved a setting off its default yet.</p>
                {/if}
                <p class="mt-3 text-xs text-text-tertiary">
                    Most-moved first; the other {unchangedNote(preferences.length - changed.length)}.
                </p>
            </div>
        {:else}
            <BetaEmptyState />
        {/if}
    {:else}
        <ErrorState error={settingsAdoption.error} {selection} />
    {/if}
</section>
