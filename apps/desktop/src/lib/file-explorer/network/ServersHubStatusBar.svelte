<script lang="ts">
    /**
     * The Servers list's status bar: how many servers, and the refresh hint. The whole bar
     * is the refresh button.
     */
    import ShortcutChip from '#lib/ui/ShortcutChip.svelte'
    import Trans from '#lib/intl/Trans.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { formatInteger } from '#lib/intl/number-format.js'

    interface Props {
        /** Servers, ❌ not rows: a share row is a place under one. */
        serverCount: number
        onRefresh: () => void
    }

    const { serverCount, onRefresh }: Props = $props()

    // The refresh hint's `<key>` chip: the live `pane.refresh` binding, non-clickable because
    // the whole status bar already refreshes (a nested target would double-activate).
    const snippets = { key: refreshKeyChip }
</script>

{#snippet refreshKeyChip(_children: import('svelte').Snippet)}<ShortcutChip
        commandId="pane.refresh"
        clickable={false}
        size="sm"
    />{/snippet}

<!-- Named by what it shows ("4 servers, Press ⌘R or click here to refresh"), ❌ no `aria-label`: one
     would have to hold every visible word, count and hint, to pass WCAG 2.5.3 (label in name). The
     hidden comma only gives a screen reader a pause between the count and the hint. -->
<!-- The top edge is a plain rule, the same height as a folder's usage track, and decoration only
     (no role): with no total to fill, a track would read as "0% full". -->
<div class="hub-footer" data-pane-footer>
<div class="pane-footer-edge" aria-hidden="true"></div>
<button class="hub-status-bar" onclick={onRefresh}>
    <span class="status-text"
        >{tString('servers.hub.rowCount', {
            count: serverCount,
            countText: formatInteger(serverCount),
        })}</span
    ><span class="sr-only">,</span>
    <span class="refresh-hint"><Trans key="fileExplorer.network.browser.refreshHint" {snippets} /></span>
</button>
</div>

<style>
    .hub-footer {
        grid-column: 1 / -1;
    }

    /* Vertical metrics are the shared `--pane-footer-*` set (`app.css`), so this band and a
       folder's `SelectionInfo` measure the same; the horizontal inset is the hub's own. */
    .hub-status-bar {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        width: 100%;
        padding: var(--pane-footer-padding-block) var(--spacing-sm);
        font-family: var(--font-system), sans-serif;
        font-size: var(--pane-footer-font-size);
        color: var(--color-text-secondary);
        background-color: var(--color-bg-secondary);
        border: none;
        min-height: var(--pane-footer-min-height);
        text-align: left;
    }

    .status-text {
        flex: 1 1 0;
        min-width: 0;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .refresh-hint {
        flex-shrink: 0;
        margin-left: auto;
        padding-left: var(--spacing-md);
        color: var(--color-text-tertiary);
        white-space: nowrap;
        display: inline-flex;
        align-items: center;
        gap: var(--spacing-xxs);
    }
</style>
