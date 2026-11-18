<script lang="ts">
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

<SettingsSection title={tString('settings.section.mtp')}>
    {#if anyVisible(shouldShow, 'fileOperations.mtpEnabled', 'fileOperations.mtpConnectionWarning')}
        <SectionCard>
            <SettingSwitchRow id="fileOperations.mtpEnabled" {searchQuery} />
            <SettingSwitchRow id="fileOperations.mtpConnectionWarning" {searchQuery} />
        </SectionCard>
    {/if}
</SettingsSection>
