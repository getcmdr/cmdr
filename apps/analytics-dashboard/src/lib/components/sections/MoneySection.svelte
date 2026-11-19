<!--
  Product § Money: revenue over the picker's complete days (per currency, never summed across one), all-time
  revenue, paid licenses and the paying share, license activations, who saw the upgrade nudge, and the
  range's first purchases. Every figure comes from `#lib/money.ts` over one all-time Paddle fetch.
-->
<script lang="ts">
    import type { SourceResult, DashboardSelection } from '#lib/server/types.js'
    import type { PaddleData } from '#lib/server/sources/paddle.js'
    import type { LicenseData } from '#lib/server/sources/license.js'
    import type { ActiveInstallsData } from '#lib/server/sources/active-installs.js'
    import type { SettingsAdoption } from '#lib/server/settings-defaults.js'
    import { allTimeRevenue, paidLicensesThrough, recentPurchases, revenueInPeriod } from '#lib/money.js'
    import { payingShareTile } from '#lib/product-stats.js'
    import { flagReach, upgradeNudgeFlag } from '#lib/onboarding-funnel.js'
    import { changeTone, changeToneClass, formatCurrency, formatNumber, formatPercent, formatRelativeChange, formatUtcDateTime } from '#lib/format.js'
    import InfoPopover from '../InfoPopover.svelte'
    import ErrorState from '../ErrorState.svelte'
    import ExternalLinks from '../ExternalLinks.svelte'

    interface Props {
        paddle: SourceResult<PaddleData>
        license: SourceResult<LicenseData>
        activeInstalls: SourceResult<ActiveInstallsData | null>
        settingsAdoption: SourceResult<SettingsAdoption>
        n: number
        selection: DashboardSelection
        todayIso: string
    }

    const { paddle, license, activeInstalls, settingsAdoption, n, selection, todayIso }: Props = $props()

    /** The list is for reading names off, not a ledger: past this many, say how many more. */
    const purchasesShown = 20

    const days = $derived(activeInstalls.ok ? (activeInstalls.data?.days ?? null) : null)
    const nudge = $derived(settingsAdoption.ok ? flagReach(settingsAdoption.data.settings, upgradeNudgeFlag) : null)

    function changeClass(change: number | null): string {
        return changeToneClass[changeTone(change, formatRelativeChange)]
    }
</script>

<section class="@container rounded-xl border border-border bg-surface p-4 sm:p-6">
    <h2 class="mb-4 flex items-center gap-2 text-lg font-semibold text-text-primary">
        Money
        <InfoPopover topic="money">
            <p>What came in, and who pays.</p>
            <ul>
                <li>Revenue counts complete days only and stays per currency: amounts in two currencies are never added.</li>
                <li>A transaction counts in its payout currency when Paddle reports one, else in its checkout currency.</li>
                <li>It's gross of refunds: a refunded purchase stays completed in Paddle.</li>
                <li>Purchases are first purchases (checkout or API). Renewals, card updates, and plan changes aren't.</li>
            </ul>
            <p><span class="info-label">Source:</span> Paddle, with a little webhook lag near the present.</p>
        </InfoPopover>
    </h2>

    {#if paddle.ok}
        {@const txns = paddle.data.allTransactions}
        {@const period = revenueInPeriod(txns, n, todayIso)}
        {@const allTime = allTimeRevenue(txns, todayIso)}
        {@const paidLicenses = paidLicensesThrough(txns, todayIso)}
        {@const paying = days ? payingShareTile(txns, days, n, todayIso) : null}
        {@const purchases = recentPurchases(txns, n, todayIso)}

        <div class="grid grid-cols-2 gap-x-6 gap-y-4 @xl:grid-cols-3 @5xl:grid-cols-5">
            <div>
                <p class="text-xs text-text-tertiary">Revenue, last {n} days</p>
                {#each period as line (line.currencyCode)}
                    <p class="text-2xl font-bold text-text-primary tabular-nums">{formatCurrency(line.amount, line.currencyCode)}</p>
                    <p class="text-xs text-text-tertiary">
                        <span class="tabular-nums {changeClass(line.change)}">{line.change === null ? '–' : formatRelativeChange(line.change)}</span>
                        vs. previous {n} days
                    </p>
                {:else}
                    <p class="text-2xl font-bold text-text-primary tabular-nums">0</p>
                {/each}
            </div>
            <div>
                <p class="text-xs text-text-tertiary">All-time revenue</p>
                {#each allTime as line (line.currencyCode)}
                    <p class="text-2xl font-bold text-text-primary tabular-nums">{formatCurrency(line.amount, line.currencyCode)}</p>
                {:else}
                    <p class="text-2xl font-bold text-text-primary tabular-nums">0</p>
                {/each}
            </div>
            <div>
                <p class="text-xs text-text-tertiary">Paid licenses</p>
                <p class="text-2xl font-bold text-text-primary tabular-nums">{formatNumber(paidLicenses)}</p>
                <p class="text-xs text-text-tertiary">
                    {paying?.value == null ? '–' : formatPercent(paying.value)} of monthly active
                </p>
            </div>
            <div>
                <div class="flex items-center gap-1.5 text-xs text-text-tertiary">
                    License activations
                    <InfoPopover topic="license activations">
                        <p>License activations, all time, and the devices the license server counts as active.</p>
                        <ul>
                            <li>Current totals: the picker doesn't change them.</li>
                        </ul>
                        <p><span class="info-label">Source:</span> the api-server's <code>/admin/stats</code>.</p>
                    </InfoPopover>
                </div>
                {#if license.ok}
                    <p class="text-2xl font-bold text-text-primary tabular-nums">{formatNumber(license.data.totalActivations)}</p>
                    <p class="text-xs text-text-tertiary">
                        {license.data.activeDevices === null ? '–' : formatNumber(license.data.activeDevices)} active devices
                    </p>
                {:else}
                    <p class="mt-2 text-sm text-text-tertiary">Couldn't load</p>
                {/if}
            </div>
            <div>
                <div class="flex items-center gap-1.5 text-xs text-text-tertiary">
                    Saw the upgrade nudge
                    <InfoPopover topic="the upgrade nudge" align="end">
                        <p>Installs that were shown the in-app upgrade nudge.</p>
                        <ul>
                            <li>Read from each install's latest heartbeat settings.</li>
                            <li>Out of the installs whose build has the nudge at all.</li>
                        </ul>
                        <p><span class="info-label">Source:</span> the app's heartbeat settings, via the api-server.</p>
                    </InfoPopover>
                </div>
                {#if !settingsAdoption.ok}
                    <p class="mt-2 text-sm text-text-tertiary">Couldn't load</p>
                {:else if nudge === null}
                    <p class="mt-2 text-sm text-text-tertiary">Not available yet</p>
                {:else}
                    <p class="text-2xl font-bold text-text-primary tabular-nums">
                        {nudge.share === null ? `${formatNumber(nudge.reached)} of ${formatNumber(nudge.eligible)}` : formatPercent(nudge.share)}
                    </p>
                    {#if nudge.share !== null}
                        <p class="text-xs text-text-tertiary tabular-nums">{formatNumber(nudge.reached)} of {formatNumber(nudge.eligible)}</p>
                    {/if}
                {/if}
            </div>
        </div>

        <h3 class="mt-6 mb-2 text-sm font-medium text-text-secondary">Purchases, last {n} days</h3>
        {#if purchases.length > 0}
            <table class="w-full max-w-md text-left text-sm">
                <tbody>
                    {#each purchases.slice(0, purchasesShown) as purchase (purchase.id)}
                        <tr class="border-b border-border-subtle/50">
                            <td class="py-1.5 pr-4 text-text-primary tabular-nums">
                                {formatUtcDateTime(purchase.createdAt)}
                                {#if purchase.isToday}<span class="ml-1 text-xs text-text-tertiary">today</span>{/if}
                            </td>
                            <td class="py-1.5 text-right text-text-secondary tabular-nums">
                                {formatCurrency(purchase.amount.amount, purchase.amount.currencyCode)}
                            </td>
                        </tr>
                    {/each}
                </tbody>
            </table>
            {#if purchases.length > purchasesShown}
                <p class="mt-1 text-xs text-text-tertiary">and {formatNumber(purchases.length - purchasesShown)} more</p>
            {/if}
        {:else}
            <p class="text-sm text-text-tertiary">No purchases in this range.</p>
        {/if}

        <ExternalLinks links={[{ label: 'View in Paddle', href: 'https://vendors.paddle.com' }]} />
    {:else}
        <ErrorState error={paddle.error} {selection} />
    {/if}
</section>
