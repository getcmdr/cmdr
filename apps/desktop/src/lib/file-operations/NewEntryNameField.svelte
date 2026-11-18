<script lang="ts">
    import { onDestroy, onMount, tick } from 'svelte'
    import TextInput from '#lib/ui/TextInput.svelte'
    import Trans from '#lib/intl/Trans.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import type { NamedKind } from './mutation-error-messages'
    import type { NewEntryNameCheck } from './new-entry-name-check.svelte'
    import ReportOffer from '#lib/error-reporter/ReportOffer.svelte'
    import { deriveTabLabel } from '#lib/file-explorer/tabs/tab-label.js'
    import { getVolumes } from '#lib/stores/volume-store.svelte.js'

    /**
     * The "Create <kind> in <dir>" subtitle plus the name field the New folder and
     * New file dialogs share. Owns the field's lifecycle: focus and select on mount,
     * validate a pre-filled name, re-validate on every keystroke and directory diff.
     */
    interface Props {
        /** Picks the copy (subtitle, aria label, placeholder) and the error element's id. */
        kind: NamedKind
        /** The directory the entry lands in, named in the subtitle the way its tab names it. */
        currentPath: string
        /** The pane's volume, so a share's root reads as the share (`deriveTabLabel`), not its mount dir. */
        volumeId?: string
        /** The validation state this field shows and drives. */
        check: NewEntryNameCheck
        /** The name being typed. Optional like every other bindable in the house set
            (`TextInput`, `TextArea`): a required prop makes its `$bindable()` read as a
            useless default, and the autofix for that silently removes the binding. */
        value?: string
        /** The underlying input, for a parent that needs to hand focus back to it. */
        inputElement?: HTMLInputElement
        /** Enter pressed in the field. */
        onSubmit: () => void
    }

    /* eslint-disable prefer-const -- the two `$bindable()` props need `let` */
    let { kind, currentPath, volumeId, check, value = $bindable(''), inputElement = $bindable(), onSubmit }: Props = $props()
    /* eslint-enable prefer-const */

    const COPY = {
        folder: {
            nameAria: 'fileOperations.mkdir.nameAria',
            placeholder: 'fileOperations.mkdir.placeholder',
            errorId: 'new-folder-error',
            warningId: 'new-folder-warning',
        },
        file: {
            nameAria: 'fileOperations.mkfile.nameAria',
            placeholder: 'fileOperations.mkfile.placeholder',
            errorId: 'new-file-error',
            warningId: 'new-file-warning',
        },
    } as const

    const copy = $derived(COPY[kind])
    const currentDirName = $derived(
        deriveTabLabel(
            currentPath,
            getVolumes().find((v) => v.id === volumeId),
        ),
    )

    onMount(async () => {
        await tick()
        inputElement?.focus()
        inputElement?.select()

        // Validate the initial name if pre-filled
        if (value.trim()) {
            void check.validate(value)
        }

        await check.listen()
    })

    onDestroy(() => {
        check.dispose()
    })

    function handleKeydown(event: KeyboardEvent) {
        if (event.key === 'Enter') {
            event.preventDefault()
            event.stopPropagation()
            onSubmit()
        }
    }
</script>

<p class="subtitle">
    {#if kind === 'folder'}
        <Trans key="fileOperations.mkdir.createIn" params={{ name: currentDirName }} snippets={{ dir }} />
    {:else}
        <Trans key="fileOperations.mkfile.createIn" params={{ name: currentDirName }} snippets={{ dir }} />
    {/if}
</p>

<div class="input-group">
    <TextInput
        bind:inputElement
        bind:value
        invalid={!!check.errorMessage}
        ariaLabel={tString(copy.nameAria)}
        aria-describedby={check.errorMessage ? copy.errorId : check.warningMessage ? copy.warningId : undefined}
        spellcheck={false}
        autocomplete="off"
        placeholder={tString(copy.placeholder)}
        onkeydown={handleKeydown}
        oninput={() => {
            check.schedule()
        }}
    />
    {#if check.errorMessage}
        <p id={copy.errorId} class="error-message" role="alert">{check.errorMessage}</p>
    {:else if check.warningMessage}
        <!-- Not an error: OK still creates it. A status, so it's read without interrupting. -->
        <p id={copy.warningId} class="warning-message" role="status">{check.warningMessage}</p>
    {/if}
    {#if check.refusal}
        <div class="refusal-report"><ReportOffer incident={check.refusal} size="mini" /></div>
    {/if}
</div>

{#snippet dir(children: import('svelte').Snippet)}<span class="dir-name">{@render children()}</span>{/snippet}

<style>
    .refusal-report {
        margin-top: var(--spacing-xs);
    }

    .subtitle {
        margin: 0 0 var(--spacing-lg);
        font-size: var(--font-size-md);
        color: var(--color-text-secondary);
    }

    .dir-name {
        color: var(--color-text-primary);
        font-weight: 500;
    }

    .input-group {
        margin-bottom: var(--spacing-lg);
    }

    .error-message,
    .warning-message {
        margin: var(--spacing-sm) 0 0;
        font-size: var(--font-size-sm);
        color: var(--color-error);
    }

    /* The darker warning text, which holds contrast on the dialog's background. */
    .warning-message {
        color: var(--color-warning-text);
    }
</style>
