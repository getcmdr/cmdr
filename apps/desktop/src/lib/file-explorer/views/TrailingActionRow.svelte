<!--
    The trailing action row both views draw after their last entry (`trailing-row.ts`): a
    "+" and the kind's words, quiet and italic like an action, never a file. It's an option
    in the listbox (the cursor and screen readers reach it) that's never marked, never a drop
    target, and never dragged: its press only lands the cursor, and its activation (through
    the view's `row-activation.ts`) runs the kind's command.
-->
<script lang="ts">
    import Icon from '#lib/ui/Icon.svelte'
    import { fileRowAttributes } from './file-row-a11y'
    import type { TrailingRow } from './trailing-row'

    interface Props {
        row: TrailingRow
        listId: string
        /** Its UI index: the entry rows' count. */
        index: number
        /** Every row the listbox holds, this one included (`aria-setsize`). */
        rowCount: number
        isUnderCursor: boolean
        isStriped: boolean
        rowHeight: number
        /** The press: lands the cursor (no modifier ever selects it). */
        onPress: () => void
        onPointerDown: (event: PointerEvent) => void
        onClick: () => void
        onDoubleClick: () => void
    }

    const { row, listId, index, rowCount, isUnderCursor, isStriped, rowHeight, onPress, onPointerDown, onClick,
        onDoubleClick }: Props = $props()
</script>

<div
    {...fileRowAttributes({ listId, globalIndex: index, totalCount: rowCount, isMarked: false, name: row.label })}
    class="file-entry trailing-row"
    class:is-under-cursor={isUnderCursor}
    class:is-striped={isStriped}
    data-trailing-row
    style="height: {rowHeight}px;"
    onpointerdown={onPointerDown}
    onmousedown={(e: MouseEvent) => {
        if (e.button === 0) onPress()
    }}
    onclick={onClick}
    ondblclick={onDoubleClick}
    oncontextmenu={(e: MouseEvent) => {
        e.preventDefault()
    }}
    role="option"
>
    <span class="trailing-icon" aria-hidden="true"><Icon name="plus" size={16} /></span>
    <span class="trailing-label">{row.label}</span>
</div>

<style>
    /* The box both views give a `.file-entry` (their own rules are scoped to them); the cursor
       and stripe chrome come from `src/app-file-list.css`. */
    .trailing-row {
        display: flex;
        align-items: center;
        box-sizing: border-box;
        padding: var(--spacing-xxs) var(--spacing-sm);
        gap: var(--spacing-sm);
        white-space: nowrap;
    }

    .trailing-icon {
        display: inline-flex;
        flex-shrink: 0;
        color: var(--color-text-tertiary);
    }

    .trailing-label {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--color-text-tertiary);
        font-style: italic;
    }
</style>
