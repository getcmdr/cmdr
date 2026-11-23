<!--
  Test-only host for the Search dialog inside its `DialogBoundary`, in the shape
  `routes/(main)/+page.svelte` mounts it: the boundary wraps the `{#if}`, and its
  `onClose` is what runs when the dialog throws.

  `view` is a `$state` object the test owns, so it can reopen the dialog between
  flushes. `onCrash` is the test's, standing in for the page's crash handler.
-->
<script lang="ts">
    import DialogBoundary from '#lib/ui/boundary/DialogBoundary.svelte'
    import SearchDialog from '#lib/search/SearchDialog.svelte'

    interface Props {
        view: { open: boolean }
        onCrash: () => void
    }

    const { view, onCrash }: Props = $props()
</script>

<p data-testid="window-alive">The window is still here</p>
<DialogBoundary name="search" onClose={onCrash}>
    {#if view.open}
        <SearchDialog
            onNavigate={() => {}}
            onClose={() => {
                view.open = false
            }}
            scopePresets={{ currentFolder: '/Users/test', currentFolderUnavailableReason: '', volumeRoot: '/' }}
        />
    {/if}
</DialogBoundary>
