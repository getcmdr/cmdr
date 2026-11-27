<script lang="ts">
    // A shell: the window's content is `DebugPage.svelte`, loaded in dev builds only (`load-debug-window.ts`).
    import { loadDebugWindow } from './load-debug-window'

    const debugPage = loadDebugWindow()
</script>

{#if debugPage}
    {#await debugPage then { default: DebugPage }}
        <DebugPage />
    {/await}
{/if}
