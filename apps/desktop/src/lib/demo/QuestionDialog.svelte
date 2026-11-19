<script lang="ts">
    /**
     * The dialog plugin's question (`plugin:dialog|message`) in the app's own modal, over the frame
     * that asked, as the desktop's native alert sits over its window. `question-dialog.ts` mounts it
     * and answers the clicked button's name. See `DETAILS.md` § Dialogs.
     */
    import ModalDialog from '#lib/ui/ModalDialog.svelte'
    import Button from '#lib/ui/Button.svelte'
    import type { Question } from './question-dialog.js'

    interface Props {
        question: Question
        onAnswer: (answer: string) => void
    }

    const { question, onAnswer }: Props = $props()
    const uid = $props.id()
</script>

<ModalDialog
    titleId="{uid}-title"
    role="alertdialog"
    ariaDescribedby="{uid}-message"
    onclose={() => {
        onAnswer(question.escapeAnswer)
    }}
    containerStyle="width: 420px"
    topmost
>
    {#snippet title()}{question.title}{/snippet}

    <p id="{uid}-message" class="message">{question.message}</p>

    {#snippet footer()}
        {#each question.buttons as button (button.answer)}
            <!-- The confirming button is the default: Return presses it (`ModalDialog`). -->
            <Button
                variant={button.isDefault ? 'primary' : 'secondary'}
                isDefault={button.isDefault}
                onclick={() => {
                    onAnswer(button.answer)
                }}>{button.label}</Button
            >
        {/each}
    {/snippet}
</ModalDialog>

<style>
    .message {
        margin: 0;
        font-size: var(--font-size-md);
        color: var(--color-text-secondary);
        white-space: pre-line;
    }
</style>
