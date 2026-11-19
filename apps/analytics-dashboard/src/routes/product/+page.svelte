<!--
  Product page (`/product`): what needs a reply and the headline tiles, full width, then a one-to-three
  column grid of usage, retention, release rollout, money, onboarding, feature usage, the feedback inbox,
  and settings adoption. Every stat reads complete days only (today is partial: faded on the charts, in no number). The shared range
  picker and nav live in the layout; this page renders the sections from its own loaded subset.
-->
<script lang="ts">
    import type { PageProps } from './$types'
    import { rangeDays, viewWindow } from '#lib/time-range.js'
    import { latestRelease } from '#lib/rollout.js'
    import NeedsYouStrip from '#lib/components/sections/NeedsYouStrip.svelte'
    import ProductHeadline from '#lib/components/sections/ProductHeadline.svelte'
    import UsageTrendsSection from '#lib/components/sections/UsageTrendsSection.svelte'
    import ReleaseRolloutSection from '#lib/components/sections/ReleaseRolloutSection.svelte'
    import RetentionCohortsSection from '#lib/components/sections/RetentionCohortsSection.svelte'
    import MoneySection from '#lib/components/sections/MoneySection.svelte'
    import OnboardingFunnelSection from '#lib/components/sections/OnboardingFunnelSection.svelte'
    import FeatureUsageSection from '#lib/components/sections/FeatureUsageSection.svelte'
    import SettingsAdoptionSection from '#lib/components/sections/SettingsAdoptionSection.svelte'
    import FeedbackErrorsSection from '#lib/components/sections/FeedbackErrorsSection.svelte'

    const { data }: PageProps = $props()

    /** Today's UTC day: partial, so the charts fade it and no stat reads it. */
    const todayIso = new Date().toISOString().slice(0, 10)

    /** The picker's range in days: the charts' span and every "vs. previous" comparison. */
    const n = $derived(rangeDays[data.selection.range])
    /** The product page has no ‹ › steps, so this is always the range ending today. */
    const shown = $derived(viewWindow(data.selection.range, null, todayIso))

    const actives = $derived(data.activeInstalls.ok ? data.activeInstalls.data : null)
    const release = $derived(latestRelease(data.github.ok ? data.github.data.releases : null, actives?.days ?? []))
    const versionWindow = $derived(actives?.windows.find((w) => w.days === n) ?? null)
</script>

<div class="flex flex-col gap-6">
    <NeedsYouStrip feedbackAndErrors={data.feedbackAndErrors} {release} />
    <ProductHeadline
        activeInstalls={data.activeInstalls}
        funnel={data.funnel}
        paddle={data.paddle}
        cloudflare={data.cloudflare}
        {n}
        {todayIso}
    />
    <!--
      One column, two from 1280px (`xl`), three from 1920px (`3xl`). The order and spans fill every row in
      both layouts with no hole, so the reading order is the DOM order: DETAILS.md § "Product page".
    -->
    <div class="grid grid-cols-1 gap-6 *:*:h-full *:min-w-0 xl:grid-cols-2 3xl:grid-cols-3">
        <div>
            <UsageTrendsSection
                activeInstalls={data.activeInstalls}
                cloudflare={data.cloudflare}
                window={shown}
                selection={data.selection}
                {todayIso}
            />
        </div>
        <div>
            <RetentionCohortsSection funnel={data.funnel} window={shown} selection={data.selection} {todayIso} />
        </div>
        <div>
            <MoneySection
                paddle={data.paddle}
                license={data.license}
                activeInstalls={data.activeInstalls}
                settingsAdoption={data.settingsAdoption}
                {n}
                selection={data.selection}
                {todayIso}
            />
        </div>
        <div>
            <OnboardingFunnelSection settingsAdoption={data.settingsAdoption} selection={data.selection} />
        </div>
        <div class="xl:col-span-2">
            <ReleaseRolloutSection activeInstalls={data.activeInstalls} {release} window={shown} selection={data.selection} {todayIso} />
        </div>
        <div class="xl:col-span-2">
            <FeatureUsageSection featureUsage={data.featureUsage} selection={data.selection} />
        </div>
        <div class="xl:col-span-2 3xl:col-span-1">
            <FeedbackErrorsSection
                feedbackAndErrors={data.feedbackAndErrors}
                {versionWindow}
                selection={data.selection}
            />
        </div>
        <div class="col-span-full">
            <SettingsAdoptionSection settingsAdoption={data.settingsAdoption} selection={data.selection} />
        </div>
    </div>
</div>
