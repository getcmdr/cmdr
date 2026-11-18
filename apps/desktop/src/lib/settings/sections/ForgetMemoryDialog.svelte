<script lang="ts">
    /**
     * "Forget everything Cmdr remembers about you?" confirmation, opened from
     * `AskCmdrSection`.
     *
     * Deleting is quick enough that there is no in-flight state to model, but both
     * buttons still go dead once the confirm has fired, so a double Enter can't
     * send a second delete at a folder the first one is walking.
     */
    import ModalDialog from '#lib/ui/ModalDialog.svelte'
    import Button from '#lib/ui/Button.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'

    interface Props {
        /** True while the delete is running. */
        isForgetting: boolean
        onConfirm: () => void
        onCancel: () => void
    }

    const { isForgetting, onConfirm, onCancel }: Props = $props()
    /** The message is the alertdialog's description, read with its title. */
    const uid = $props.id()
    const messageId = `${uid}-message`
</script>

<ModalDialog
    titleId="forget-memory-title"
    dialogId="forget-memory"
    role="alertdialog"
    ariaDescribedby={messageId}
    onclose={() => {
        if (!isForgetting) onCancel()
    }}
    containerStyle="width: 420px"
>
    {#snippet title()}{tString('askCmdr.forget.title')}{/snippet}
    <p class="confirm-message" id={messageId}>{tString('askCmdr.forget.message')}</p>
    {#snippet footer()}
        <Button variant="secondary" disabled={isForgetting} onclick={onCancel}>
            {tString('askCmdr.forget.cancel')}
        </Button>
        <!-- The default button (Return presses it), as before: the person chose to forget. A
             danger default means Return on Cancel never forgets instead (`ModalDialog`). -->
        <Button variant="danger" isDefault disabled={isForgetting} onclick={onConfirm}>
            {tString('askCmdr.forget.confirm')}
        </Button>
    {/snippet}
</ModalDialog>

<style>
    .confirm-message {
        margin: 0;
        font-size: var(--font-size-md);
        line-height: var(--font-line-height-prose);
        color: var(--color-text-secondary);
    }
</style>
