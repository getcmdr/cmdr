<script lang="ts">
    /**
     * The question a queued operation asks when some of its sources left while it
     * waited: "4 of the 22 items this move was waiting on were moved or deleted
     * since you queued it", with "Move the rest" and Cancel.
     *
     * Presentational: `sources-gone.svelte.ts` owns the question and the answers,
     * and `SourcesGonePrompt.svelte` binds the two. No `onclose`, so no × and no
     * Escape: both ways out decide something about the operation, and a reflex
     * Escape on a prompt that appeared on its own shouldn't pick one.
     */
    import ModalDialog from '#lib/ui/ModalDialog.svelte'
    import Button from '#lib/ui/Button.svelte'
    import { enterPressesFocusedButton } from '#lib/ui/dialog-keys.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { formatInteger } from '#lib/intl/number-format.js'
    import { tooltip } from '#lib/tooltip/tooltip.js'
    import type { OperationSnapshot, WriteSourcesGoneEvent } from '#lib/tauri-commands/index.js'
    import { getFolderName } from './transfer/transfer-dialog-utils'

    interface Props {
        /** The backend's question. */
        event: WriteSourcesGoneEvent
        /** The operation's queue row, for naming which transfer is asking. */
        snapshot: OperationSnapshot | null
        /** An answer is in flight: both buttons disable. */
        answering: boolean
        /** Carry on with the items still there. */
        onContinue: () => void
        /** End the operation. */
        onCancel: () => void
    }

    const { event, snapshot, answering, onContinue, onCancel }: Props = $props()

    /** Which operation is asking, in the conflict prompt's own words: several can
     *  be queued at once, and the buttons below act on exactly one. */
    const context = $derived(
        snapshot === null
            ? null
            : tString('fileOperations.operationConflict.context', {
                  type: snapshot.operationType,
                  hasDestination: snapshot.destination === null ? 'no' : 'yes',
                  destination: snapshot.destination === null ? '' : getFolderName(snapshot.destination),
              }),
    )

    const message = $derived(
        tString('fileOperations.sourcesGone.message', {
            type: event.operationType,
            gone: event.goneCount,
            goneText: formatInteger(event.goneCount),
            totalText: formatInteger(event.totalCount),
        }),
    )

    /** The gone items the backend didn't name: the count is the whole truth, the
     *  names only help the person recognize what moved. */
    const unnamed = $derived(Math.max(0, event.goneCount - event.goneSample.length))
</script>

<ModalDialog
    dialogId="sources-gone"
    titleId="sources-gone-title"
    ariaDescribedby="sources-gone-message"
    containerStyle="width: 460px; min-width: 360px"
    resizable="horizontal"
>
    {#snippet title()}{tString('fileOperations.sourcesGone.title')}{/snippet}

    {#if context !== null}
        <p class="context">{context}</p>
    {/if}

    <p id="sources-gone-message" class="message">{message}</p>

    <ul class="gone-list">
        {#each event.goneSample as path (path)}
            <li class="gone-name" use:tooltip={{ text: path, overflowOnly: true }}>{getFolderName(path)}</li>
        {/each}
        {#if unnamed > 0}
            <li class="gone-more">
                {tString('fileOperations.sourcesGone.moreItems', {
                    count: unnamed,
                    countText: formatInteger(unnamed),
                })}
            </li>
        {/if}
    </ul>

    {#snippet footer()}
        <Button variant="secondary" disabled={answering} onclick={onCancel}
            >{tString('fileOperations.button.cancel')}</Button
        >
        <!-- Doing the rest is the answer people mean nearly every time, and it
             overwrites nothing that wasn't already going to be written, so it's
             the default button, the one Return presses. -->
        <Button
            variant="primary"
            isDefault
            autoFocus={enterPressesFocusedButton()}
            disabled={answering}
            onclick={onContinue}
            >{tString('fileOperations.sourcesGone.continue', { type: event.operationType })}</Button
        >
    {/snippet}
</ModalDialog>

<style>
    .context {
        margin: 0 0 var(--spacing-xs);
        font-size: var(--font-size-sm);
        color: var(--color-text-secondary);
    }

    .message {
        margin: 0 0 var(--spacing-sm);
        font-size: var(--font-size-md);
    }

    .gone-list {
        margin: 0;
        padding: 0;
        list-style: none;
        font-size: var(--font-size-sm);
        color: var(--color-text-secondary);
    }

    .gone-name {
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
    }

    .gone-more {
        color: var(--color-text-tertiary);
    }
</style>
