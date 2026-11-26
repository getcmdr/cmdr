<script lang="ts">
    import { onMount } from 'svelte'
    import SectionCard from '#lib/ui/SectionCard.svelte'
    import Icon from '#lib/ui/Icon.svelte'
    import FileIcon from '#lib/file-explorer/selection/FileIcon.svelte'
    import type { FileEntry } from '#lib/file-explorer/types'
    import { prefetchIcons } from '#lib/icon-cache.js'
    import { getUseAppIconsForDocuments } from '#lib/settings/reactive-settings.svelte.js'
    import GlyphGrid from '../GlyphGrid.svelte'

    interface RowIcon {
        id: 'folder' | 'share' | 'smb-server'
        caption: string
        usage: string
    }

    const ROW_ICONS: RowIcon[] = [
        {
            id: 'folder',
            caption: 'dir',
            usage: 'A plain folder row: the OS folder icon, gold under the Cmdr-gold folder setting.',
        },
        {
            id: 'share',
            caption: 'network:share',
            usage: 'A share row on a server’s share list: the OS folder icon twice, stacked, gold like a folder.',
        },
        {
            id: 'smb-server',
            caption: 'monitor-filled',
            usage: 'An SMB server row in the Servers list, in `--color-server-icon`.',
        },
    ]

    /** A row entry carrying `iconId`, for drawing through the real `FileIcon`. */
    function rowEntry(iconId: string): FileEntry {
        return {
            name: iconId,
            path: `/graphics-catalog/${iconId}`,
            isDirectory: true,
            isSymlink: false,
            permissions: 0,
            owner: '',
            group: '',
            iconId,
        }
    }

    // The catalog may open before any pane asked for the folder icon.
    onMount(() => {
        void prefetchIcons(['dir', 'network:share'], getUseAppIconsForDocuments())
    })
</script>

<SectionCard id="graphics-row-icons" label="Row icons">
    <GlyphGrid items={ROW_ICONS}>
        {#snippet intro()}
            Row icons as a file list draws them (the Servers list too): the real size beside a 3× zoom. Folder and share
            icons are the OS's own pixels, so they follow the platform, the theme, and the folder-color setting.
        {/snippet}
        {#snippet glyph(item: RowIcon)}
            <span class="pair">
                {#if item.id === 'smb-server'}
                    <span class="server-glyph"><Icon name="monitor-filled" size={16} /></span>
                    <span class="server-glyph"><Icon name="monitor-filled" size={48} /></span>
                {:else}
                    {@const entry = rowEntry(item.id === 'share' ? 'network:share' : 'dir')}
                    <FileIcon file={entry} />
                    <span class="zoomed"><FileIcon file={entry} /></span>
                {/if}
            </span>
        {/snippet}
    </GlyphGrid>
</SectionCard>

<style>
    .pair {
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-lg);
    }

    /* A 3× zoom of the 16 px icon, laid out at its zoomed size. */
    .zoomed {
        display: inline-flex;
        zoom: 3;
    }

    .server-glyph {
        display: inline-flex;
        color: var(--color-server-icon);
    }
</style>
