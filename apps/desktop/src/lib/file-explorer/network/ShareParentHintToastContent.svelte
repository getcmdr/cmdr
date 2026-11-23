<script lang="ts">
    /**
     * The once-ever INFO toast raised the first time `..` takes someone out of an SMB
     * share's top folder into the server's share list (#1238): what happened, and two
     * ways to turn it off right here.
     *
     * `share-parent-row.svelte.ts::showShareParentHintOnce` raises it and sets
     * `behavior.shareParentRowHintSeen`; this component only answers its buttons.
     *
     * ❗ Buttons in macOS order (dismissive first, primary last), the order every house
     * button row is written in (`#lib/ui/DETAILS.md` § Button order).
     */
    import Button from '#lib/ui/Button.svelte'
    import ToastActions from '#lib/ui/toast/ToastActions.svelte'
    import { dismissToast } from '#lib/ui/toast/index.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { setShareParentRow, turnOffShareParentRowForAll } from './share-parent-row.svelte'

    interface Props {
        /** Dedup id of this toast, so each button can retire it. */
        toastId: string
        /** The share `..` was used from, which "Turn off for this share" acts on. */
        volumeId: string
        /** The server's name in words. */
        serverLabel: string
    }

    const { toastId, volumeId, serverLabel }: Props = $props()

    async function turnOffForShare(): Promise<void> {
        dismissToast(toastId)
        await setShareParentRow(volumeId, false)
    }

    function turnOffForAll(): void {
        dismissToast(toastId)
        turnOffShareParentRowForAll()
    }
</script>

<div class="content">
    <span class="body">{tString('fileExplorer.shareParentRow.hint', { server: serverLabel })}</span>
    <ToastActions>
        <Button size="mini" variant="secondary" onclick={() => void turnOffForShare()}
            >{tString('fileExplorer.shareParentRow.turnOffForShare')}</Button
        >
        <Button size="mini" variant="secondary" onclick={turnOffForAll}
            >{tString('fileExplorer.shareParentRow.turnOffForAll')}</Button
        >
        <Button
            size="mini"
            variant="primary"
            onclick={() => {
                dismissToast(toastId)
            }}>{tString('fileExplorer.shareParentRow.gotIt')}</Button
        >
    </ToastActions>
</div>

<style>
    .content {
        font-size: var(--font-size-sm);
    }

    .body {
        color: var(--color-text-primary);
    }
</style>
