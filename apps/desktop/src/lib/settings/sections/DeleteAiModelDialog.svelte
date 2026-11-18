<script lang="ts">
    /**
     * "Delete the local AI model?" confirmation, opened from `AiLocalSection`.
     *
     * Mid-delete the whole dialog changes: the title, a spinner body instead of
     * the size warning, and both buttons disabled (Escape and Enter included) so
     * an uninstall in flight can't be cancelled or double-fired.
     */
    import ModalDialog from '#lib/ui/ModalDialog.svelte'
    import Button from '#lib/ui/Button.svelte'
    import Spinner from '#lib/ui/Spinner.svelte'
    import { t, tString } from '#lib/intl/messages.svelte.js'

    interface Props {
        /** The installed model's size, already formatted (`formatByteSize`). */
        modelSizeFormatted: string
        /** True while the uninstall is running. */
        isDeleting: boolean
        onConfirm: () => void
        onCancel: () => void
    }

    const { modelSizeFormatted, isDeleting, onConfirm, onCancel }: Props = $props()
    /** Whichever body shows (the size warning, or the progress line) is the alertdialog's description. */
    const uid = $props.id()
    const messageId = `${uid}-message`
</script>

<ModalDialog
    titleId="delete-ai-model-title"
    dialogId="delete-ai-model"
    role="alertdialog"
    ariaDescribedby={messageId}
    onclose={() => {
        if (!isDeleting) onCancel()
    }}
    containerStyle="width: 400px"
>
    {#snippet title()}{isDeleting
            ? tString('ai.local.deleteDialogTitleDeleting')
            : tString('ai.local.deleteDialogTitle')}{/snippet}
    {#if isDeleting}
        <div class="deleting-status" id={messageId}>
            <Spinner size="sm" />
            <span>{tString('ai.local.deletingStatus')}</span>
        </div>
    {:else}
        <p class="confirm-message" id={messageId}>
            {t('ai.local.deleteConfirmMessage', { modelSize: modelSizeFormatted })}
        </p>
    {/if}
    {#snippet footer()}
        <Button variant="secondary" disabled={isDeleting} onclick={onCancel}>{tString('ai.local.cancel')}</Button>
        <!-- The default button (Return presses it), as before: the person chose to delete. A
             danger default means Return on Cancel never deletes instead (`ModalDialog`). -->
        <Button variant="danger" isDefault disabled={isDeleting} onclick={onConfirm}>
            {isDeleting ? tString('ai.local.deleteButtonDeleting') : tString('ai.local.deleteButton')}
        </Button>
    {/snippet}
</ModalDialog>

<style>
    .confirm-message {
        margin: 0;
        font-size: var(--font-size-md);
        color: var(--color-text-secondary);
    }

    .deleting-status {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: var(--spacing-sm);
        margin: 0 0 var(--spacing-lg);
        font-size: var(--font-size-md);
        color: var(--color-text-secondary);
    }
</style>
