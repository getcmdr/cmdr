<script lang="ts">
    import SettingsSection from '../components/SettingsSection.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import SettingRow from '../components/SettingRow.svelte'
    import SettingSlider from '../components/SettingSlider.svelte'
    import SettingSwitch from '../components/SettingSwitch.svelte'
    import SettingToggleGroup from '../components/SettingToggleGroup.svelte'
    import SectionCard from '#lib/ui/SectionCard.svelte'
    import { getSettingDefinition } from '#lib/settings/index.js'
    import { createShouldShow, anyVisible } from '#lib/settings/settings-search.js'

    interface Props {
        searchQuery: string
    }

    const { searchQuery }: Props = $props()

    const shouldShow = $derived(createShouldShow(searchQuery))

    const textSizeDef = getSettingDefinition('appearance.textSize') ?? { label: '', description: '' }
    const uiDensityDef = getSettingDefinition('appearance.uiDensity') ?? { label: '', description: '' }
    const fnKeyBarDef = getSettingDefinition('appearance.showFunctionKeyBar') ?? { label: '', description: '' }
</script>

<SettingsSection title={tString('settings.section.zoomAndDensity')}>
    {#if anyVisible(shouldShow, 'appearance.textSize', 'appearance.uiDensity', 'appearance.showFunctionKeyBar')}
        <SectionCard>
            {#if shouldShow('appearance.textSize')}
                <SettingRow
                    id="appearance.textSize"
                    label={textSizeDef.label}
                    description={textSizeDef.description}
                    {searchQuery}
                >
                    <SettingSlider id="appearance.textSize" unit="%" />
                </SettingRow>
            {/if}

            {#if shouldShow('appearance.uiDensity')}
                <SettingRow
                    id="appearance.uiDensity"
                    label={uiDensityDef.label}
                    description={uiDensityDef.description}
                    {searchQuery}
                >
                    <SettingToggleGroup id="appearance.uiDensity" />
                </SettingRow>
            {/if}

            <!-- The bar spends a row of the window's height, so it sits with the other
                 "how much fits on screen" choices. -->
            {#if shouldShow('appearance.showFunctionKeyBar')}
                <SettingRow
                    id="appearance.showFunctionKeyBar"
                    label={fnKeyBarDef.label}
                    description={fnKeyBarDef.description}
                    {searchQuery}
                >
                    <SettingSwitch id="appearance.showFunctionKeyBar" />
                </SettingRow>
            {/if}
        </SectionCard>
    {/if}
</SettingsSection>
