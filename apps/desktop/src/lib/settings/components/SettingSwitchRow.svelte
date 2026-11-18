<script lang="ts">
    /**
     * A registry on/off setting as one whole row: the registry's label and description, a
     * `SettingSwitch`, and the same `shouldShow` search guard every hand-written row carries.
     * Reach for it when the row needs nothing else (no badge, note, or extra control); the
     * section still owns its card and the card's `anyVisible(...)` gate.
     */
    import { getSettingDefinition, type SettingId } from '#lib/settings/index.js'
    import { createShouldShow } from '#lib/settings/settings-search.js'
    import SettingRow from './SettingRow.svelte'
    import SettingSwitch from './SettingSwitch.svelte'

    interface Props {
        id: SettingId
        searchQuery: string
    }

    const { id, searchQuery }: Props = $props()

    const definition = $derived(getSettingDefinition(id))
    const visible = $derived(createShouldShow(searchQuery)(id))
</script>

{#if visible}
    <SettingRow {id} label={definition?.label ?? ''} description={definition?.description ?? ''} {searchQuery}>
        <SettingSwitch {id} />
    </SettingRow>
{/if}
