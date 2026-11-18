<script lang="ts">
    /**
     * "Delete all Cmdr data on this Mac?", opened from `UpdatesSection`.
     *
     * Three views. Confirm says what goes and what stays, with Cancel focused, so Enter or
     * Escape walks away. Deleting can't be closed or re-fired: the backend is mid-wipe and a
     * half-wiped state is worse than either end. Done offers only "Quit Cmdr", and closing it
     * quits too: an app left running after its data is gone would write a fresh state back.
     */
    import ModalDialog from '#lib/ui/ModalDialog.svelte'
    import Button from '#lib/ui/Button.svelte'
    import { enterPressesFocusedButton } from '#lib/ui/dialog-keys.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { goesItems, introText, leftoverText, refusalText, type DataWipeView } from './delete-all-data'

    interface Props {
        view: DataWipeView
        onConfirm: () => void
        onCancel: () => void
        onQuit: () => void
    }

    const { view, onConfirm, onCancel, onQuit }: Props = $props()

    /** The intro (or the outcome) is the alertdialog's description, read with its title. */
    const uid = $props.id()
    const messageId = `${uid}-message`

    const deleting = $derived(view.kind === 'deleting')
    const leftovers = $derived(view.kind === 'done' ? view.report.leftovers : [])
    const summary = $derived(view.kind === 'done' ? null : view.summary)
    const refusal = $derived(view.kind === 'confirm' ? view.refusal : null)

    function handleClose(): void {
        if (view.kind === 'done') onQuit()
        else if (view.kind === 'confirm') onCancel()
    }
</script>

<ModalDialog
    titleId="delete-all-data-title"
    dialogId="delete-all-data"
    role="alertdialog"
    ariaDescribedby={messageId}
    onclose={handleClose}
    containerStyle="width: 480px"
    resizable="horizontal"
>
    {#snippet title()}{view.kind !== 'done'
            ? tString('settings.dataWipe.title')
            : leftovers.length === 0
              ? tString('settings.dataWipe.doneTitle')
              : tString('settings.dataWipe.partialTitle')}{/snippet}

    {#if view.kind === 'done'}
        {#if leftovers.length === 0}
            <p class="message" id={messageId}>{tString('settings.dataWipe.doneMessage')}</p>
        {:else}
            <p class="message" id={messageId}>{tString('settings.dataWipe.partialMessage')}</p>
            <ul class="leftovers">
                {#each leftovers as leftover, i (i)}
                    <li class:path={leftover.type === 'path'}>{leftoverText(leftover)}</li>
                {/each}
            </ul>
        {/if}
    {:else}
        <p class="message" id={messageId}>{introText(summary)}</p>
        <h3 class="list-heading">{tString('settings.dataWipe.goesHeading')}</h3>
        <ul class="goes">
            {#each goesItems(summary) as item (item)}
                <li>{item}</li>
            {/each}
        </ul>
        <h3 class="list-heading">{tString('settings.dataWipe.staysHeading')}</h3>
        <p class="stays">{tString('settings.dataWipe.stays')}</p>
        {#if refusal}
            <p class="refusal" role="alert">{refusalText(refusal)}</p>
        {/if}
    {/if}

    {#snippet footer()}
        {#if view.kind === 'done'}
            <Button variant="primary" isDefault autoFocus={enterPressesFocusedButton()} onclick={onQuit}
                >{tString('settings.dataWipe.quit')}</Button
            >
        {:else}
            <!-- No default button: a reflex Return wipes nothing. Cancel takes focus only where
                 Return presses the focused button (Windows, Linux); on the Mac Return does nothing. -->
            <Button variant="secondary" autoFocus={enterPressesFocusedButton()} disabled={deleting} onclick={onCancel}>
                {tString('settings.dataWipe.cancel')}
            </Button>
            <Button variant="danger" disabled={deleting} onclick={onConfirm}>
                {deleting ? tString('settings.dataWipe.deleting') : tString('settings.dataWipe.confirm')}
            </Button>
        {/if}
    {/snippet}
</ModalDialog>

<style>
    .message,
    .stays {
        margin: 0;
        font-size: var(--font-size-md);
        color: var(--color-text-secondary);
    }

    .message {
        color: var(--color-text-primary);
    }

    .list-heading {
        margin: var(--spacing-lg) 0 var(--spacing-xs);
        font-size: var(--font-size-sm);
        font-weight: 600;
        color: var(--color-text-primary);
    }

    .goes,
    .leftovers {
        margin: 0;
        padding-left: var(--spacing-lg);
        font-size: var(--font-size-md);
        color: var(--color-text-secondary);
    }

    .leftovers {
        margin-top: var(--spacing-sm);
        user-select: text;
    }

    /* A path is the whole point of its line, so it wraps rather than truncating:
       the person needs all of it to find the folder. */
    .leftovers .path {
        font-family: var(--font-mono);
        font-size: var(--font-size-sm);
    }

    .refusal {
        margin: var(--spacing-lg) 0 0;
        font-size: var(--font-size-md);
        color: var(--color-text-primary);
    }
</style>
