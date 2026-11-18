<script lang="ts">
    /**
     * Settings > Behavior > Search.
     *
     * One card: the auto-apply toggle. The recent-searches and recent-selections
     * caps live in Advanced (their single home); this page holds only its own
     * settings, never a mirror.
     *
     * Card visibility is section-owned: the `SectionCard` frame is wrapped in
     * `{#if anyVisible(shouldShow, ...ids)}` over the SAME `shouldShow` predicate
     * that gates each row.
     */
    import SettingsSection from '../components/SettingsSection.svelte'
    import SettingSwitchRow from '../components/SettingSwitchRow.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import SectionCard from '#lib/ui/SectionCard.svelte'
    import { createShouldShow, anyVisible } from '#lib/settings/settings-search.js'

    interface Props {
        searchQuery: string
    }

    const { searchQuery }: Props = $props()

    const shouldShow = $derived(createShouldShow(searchQuery))
</script>

<SettingsSection title={tString('settings.section.search')}>
    {#if anyVisible(shouldShow, 'search.autoApply')}
        <SectionCard>
            <SettingSwitchRow id="search.autoApply" {searchQuery} />
        </SectionCard>
    {/if}
</SettingsSection>
