<!--
  Product page (`/product`): one column of full-width sections, top to bottom: what needs a reply, the
  headline tiles, usage, retention, money, feature usage, settings adoption, and the feedback inbox. Every
  stat reads complete days only (today is partial: faded on the charts, in no number). The shared range
  picker and nav live in the layout; this page renders the sections from its own loaded subset.
-->
<script lang="ts">
    import type { PageProps } from './$types'
    import { rangeDays, viewWindow } from '#lib/time-range.js'
    import { latestRelease } from '#lib/rollout.js'
    import NeedsYouStrip from '#lib/components/sections/NeedsYouStrip.svelte'
    import ProductHeadline from '#lib/components/sections/ProductHeadline.svelte'
    import UsageTrendsSection from '#lib/components/sections/UsageTrendsSection.svelte'
    import RetentionCohortsSection from '#lib/components/sections/RetentionCohortsSection.svelte'
    import MoneySection from '#lib/components/sections/MoneySection.svelte'
    import FeatureUsageSection from '#lib/components/sections/FeatureUsageSection.svelte'
    import SettingsAdoptionSection from '#lib/components/sections/SettingsAdoptionSection.svelte'
    import FeedbackErrorsSection from '#lib/components/sections/FeedbackErrorsSection.svelte'

    const { data, form }: PageProps = $props()

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
    <ProductHeadline activeInstalls={data.activeInstalls} funnel={data.funnel} paddle={data.paddle} {n} {todayIso} />
    <UsageTrendsSection
        activeInstalls={data.activeInstalls}
        cloudflare={data.cloudflare}
        window={shown}
        selection={data.selection}
        {todayIso}
    />
    <RetentionCohortsSection funnel={data.funnel} window={shown} selection={data.selection} {todayIso} />
    <MoneySection
        paddle={data.paddle}
        license={data.license}
        activeInstalls={data.activeInstalls}
        settingsAdoption={data.settingsAdoption}
        {n}
        selection={data.selection}
        {todayIso}
    />
    <FeatureUsageSection featureUsage={data.featureUsage} selection={data.selection} />
    <SettingsAdoptionSection settingsAdoption={data.settingsAdoption} selection={data.selection} />
    <FeedbackErrorsSection
        feedbackAndErrors={data.feedbackAndErrors}
        {versionWindow}
        selection={data.selection}
        answerError={form && 'error' in form ? form.error : null}
    />
</div>
