<script lang="ts">
    import SettingsSection from '../components/SettingsSection.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import SettingRow from '../components/SettingRow.svelte'
    import SettingSwitch from '../components/SettingSwitch.svelte'
    import SectionCard from '#lib/ui/SectionCard.svelte'
    import { getSettingDefinition } from '#lib/settings/index.js'
    import { createShouldShow, anyVisible } from '#lib/settings/settings-search.js'

    interface Props {
        searchQuery: string
    }

    const { searchQuery }: Props = $props()

    const shouldShow = $derived(createShouldShow(searchQuery))

    const defaultDef = { label: '', description: '' }
    const mtpEnabledDef = getSettingDefinition('fileOperations.mtpEnabled') ?? defaultDef
    const mtpWarningDef = getSettingDefinition('fileOperations.mtpConnectionWarning') ?? defaultDef
</script>

<SettingsSection title={tString('settings.section.mtp')}>
    {#if anyVisible(shouldShow, 'fileOperations.mtpEnabled', 'fileOperations.mtpConnectionWarning')}
        <SectionCard>
            {#if shouldShow('fileOperations.mtpEnabled')}
                <SettingRow
                    id="fileOperations.mtpEnabled"
                    label={mtpEnabledDef.label}
                    description={mtpEnabledDef.description}
                    {searchQuery}
                >
                    <SettingSwitch id="fileOperations.mtpEnabled" />
                </SettingRow>
            {/if}

            {#if shouldShow('fileOperations.mtpConnectionWarning')}
                <SettingRow
                    id="fileOperations.mtpConnectionWarning"
                    label={mtpWarningDef.label}
                    description={mtpWarningDef.description}
                    {searchQuery}
                >
                    <SettingSwitch id="fileOperations.mtpConnectionWarning" />
                </SettingRow>
            {/if}
        </SectionCard>
    {/if}
</SettingsSection>
