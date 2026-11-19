<script lang="ts">
    import SettingsSection from '../components/SettingsSection.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import SettingRow from '../components/SettingRow.svelte'
    import SettingSlider from '../components/SettingSlider.svelte'
    import SettingSwitch from '../components/SettingSwitch.svelte'
    import SettingToggleGroup from '../components/SettingToggleGroup.svelte'
    import SectionCard from '#lib/ui/SectionCard.svelte'
    import { getSettingDefinition, setSetting } from '#lib/settings/index.js'
    import { createShouldShow, anyVisible } from '#lib/settings/settings-search.js'
    import { hasCapability } from '#lib/platform/index.js'
    import Switch from '#lib/ui/Switch.svelte'
    import { getShowMenuBar } from '#lib/settings/reactive-settings.svelte.js'
    import { useSettingLock } from '../components/setting-lock.svelte'

    interface Props {
        searchQuery: string
    }

    const { searchQuery }: Props = $props()

    const shouldShow = $derived(createShouldShow(searchQuery))

    const textSizeDef = getSettingDefinition('appearance.textSize') ?? { label: '', description: '' }
    const uiDensityDef = getSettingDefinition('appearance.uiDensity') ?? { label: '', description: '' }
    const fnKeyBarDef = getSettingDefinition('appearance.showFunctionKeyBar') ?? { label: '', description: '' }
    const menuBarDef = getSettingDefinition('appearance.menuBar') ?? { label: '', description: '' }
    // Off macOS only: there the bar is the system's (`hideableMenuBar`).
    const offersMenuBarToggle = hasCapability('hideableMenuBar')
    // One switch over a three-value setting: on while the bar shows, the desktop's default resolved.
    // The store keeps `desktopDefault` until the first flip (`menu-layout.ts`'s `MenuBarChoice`).
    const menuBarLock = useSettingLock('appearance.menuBar')
</script>

<SettingsSection title={tString('settings.section.zoomAndDensity')}>
    {#if anyVisible(shouldShow, 'appearance.textSize', 'appearance.uiDensity', 'appearance.menuBar', 'appearance.showFunctionKeyBar')}
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

            {#if offersMenuBarToggle && shouldShow('appearance.menuBar')}
                <SettingRow
                    id="appearance.menuBar"
                    label={menuBarDef.label}
                    description={menuBarDef.description}
                    {searchQuery}
                >
                    <Switch
                        checked={getShowMenuBar()}
                        onCheckedChange={(next: boolean) => {
                            setSetting('appearance.menuBar', next ? 'shown' : 'hidden')
                        }}
                        disabled={menuBarLock.locked}
                        ariaLabel={menuBarDef.label}
                        ariaDescribedBy={menuBarLock.describedBy()}
                    />
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
