<script lang="ts">
    /**
     * The open surfaces of a command-menu stack (`menu-stack.svelte.ts`): the bar's flyout or a
     * context menu, and each submenu beside its row. Portaled, so no ancestor's clip or stacking
     * context reaches them. The behavior is the controller's; this draws, places, and moves focus.
     */
    import { Portal } from '@ark-ui/svelte/portal'
    import { usePortalTarget } from '../portal-target'
    import MenuSurface from './MenuSurface.svelte'
    import type { MenuStack } from './menu-stack.svelte'
    import type { MenuFlavor } from './rows'

    interface Props {
        stack: MenuStack
        flavor: MenuFlavor
        /** Whether the access-key underlines show (`keyboard-cues.ts`). */
        cuesShown: boolean
        /** Which owner's surfaces these are (`bar` or `context`), for the owner's outside-click test. */
        owner: 'bar' | 'context'
        /** Extra classes on every surface: a look of its own (the demo's macOS menus). */
        class?: string
    }

    const { stack, flavor, cuesShown, owner, class: lookClass = '' }: Props = $props()

    const portalTarget = usePortalTarget()

    /** Each open surface's element, by depth, so a submenu can measure its parent row. */
    const elements: (HTMLElement | undefined)[] = $state([])
</script>

{#if stack.isOpen}
    <Portal container={portalTarget()}>
        {#each stack.surfaces as surface, depth (surface.id)}
            <MenuSurface
                {stack}
                {surface}
                {depth}
                {flavor}
                {cuesShown}
                {owner}
                {lookClass}
                parentElement={depth > 0 ? elements[depth - 1] : undefined}
                childId={stack.surfaces.at(depth + 1)?.id}
                bind:element={elements[depth]}
            />
        {/each}
    </Portal>
{/if}
