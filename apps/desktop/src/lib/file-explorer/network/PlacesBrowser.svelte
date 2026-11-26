<script lang="ts">
    /**
     * The places under one account: an SMB host's shares today, a storage
     * account's buckets later. Raises the sign-in form when the listing needs one.
     */
    import { onMount, untrack } from 'svelte'
    import { dependOn } from '#lib/utils/reactivity.js'
    import Button from '#lib/ui/Button.svelte'
    import Icon from '#lib/ui/Icon.svelte'
    import CopyBox from '#lib/ui/CopyBox.svelte'
    import Spinner from '#lib/ui/Spinner.svelte'
    import type { AuthMode, PlacesAccount, ShareInfo, ShareListError } from '../types'
    import { renderShareListError } from './share-list-error-messages'
    import { shareListErrorOf } from './share-list-error'
    import {
        getShareState,
        fetchShares,
        clearShareState,
        getCredentialStatus,
        noteCachedCredentials,
        getListedAccount,
        guestListingWorked,
    } from './network-store.svelte'
    import PlacesHeader from './PlacesHeader.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { isListingAuthError, refusalForShareError } from './smb-sign-in'
    import {
        askToList,
        forgetPassword,
        isShareListSheetUp,
        readStoredCredentials,
        signInAs as signInAsOnSheet,
        tryStoredCredentials,
        useGuest as listAsGuest,
        type ShareListAttempt,
        type ShareListCredentials,
        type ShareListServer,
        type ShareListSheetOutcome,
    } from './share-list-sign-in'
    import type { ConnectRefusalKind } from '#lib/servers/connect-refusals.js'
    import { handleNavigationShortcut } from '../navigation/keyboard-shortcuts'
    import { eventMatchesCommand } from '#lib/shortcuts/index.js'
    import { claimKey } from '#lib/shortcuts/claim-key.js'
    import { cursorAfterArrow, indexOfName, nameStartsWith, scrollRowIntoView } from './list-cursor'
    import { updateLeftPaneState, updateRightPaneState, type PaneState, type PaneFileEntry } from '#lib/tauri-commands/index.js'

    /** Row height for share list (matches Full list) */
    const SHARE_ROW_HEIGHT = 20

    /**
     * ❗ Row 0 is `..`, back to the Servers list, whatever `network.showShareParentRow` says, so
     * every cursor index here counts it: share `i` is row `i + PARENT_ROWS`, like a file
     * list's parent offset. Type-to-jump and the landing match shares only.
     */
    const PARENT_ROWS = 1

    /** What `..` reads as in `cmdr://state`: the file list's `..`, pointing at the Servers list. */
    const PARENT_MCP_ENTRY: PaneFileEntry = {
        name: '..',
        path: 'smb://',
        isDirectory: true,
        size: null,
        recursiveSize: null,
        modified: null,
        recursiveSizeUpdating: null,
    }

    interface Props {
        /** Whose places these are. One SMB arm today; see `PlacesAccount`. */
        account: PlacesAccount
        /** Which pane this browser lives in (for MCP state sync) */
        paneId?: 'left' | 'right'
        /** Whether this pane is focused */
        isFocused?: boolean
        /** A share queued to mount once the list is on screen (an `smb://host/share` address, "Copy path between panes"). */
        autoMountShare?: string
        /**
         * Spends the queued share against the loaded list: answers the share to mount, or
         * `null` when the list lacks it (the owner says so). ❗ A one-shot: the owner retires
         * the queue here, so a list that comes back (after a Cancel, a refused mount) doesn't
         * mount it again.
         */
        takeAutoMount?: (shares: ShareInfo[]) => ShareInfo | null
        /** Callback when user selects a share, includes credentials if auth was used */
        onShareSelect?: (share: ShareInfo, credentials: { username: string; password: string } | null) => void
        /** Callback to go back to host list */
        onBack?: () => void
        /**
         * The share `..` came out of (#1238): the cursor lands on it once the list loads, the
         * way going up from a folder lands on the folder you left. Once per distinct value.
         */
        landOnShare?: string
        /** The list loaded and the cursor took `landOnShare` (or found no such share). */
        onLandedOnShare?: () => void
    }

    const {
        account,
        paneId,
        isFocused = false,
        autoMountShare,
        takeAutoMount,
        onShareSelect,
        onBack,
        landOnShare,
        onLandedOnShare,
    }: Props = $props()

    // The SMB host behind the account. Everything below still speaks SMB; the
    // account is the seam a second protocol arrives through, not a rewrite.
    const host = $derived(account.host)
    // ❗ What to CALL it in headers and toasts (a name the person gave it); `host.name`
    // stays the identity every key and lookup below uses.
    const hostLabel = $derived(account.label ?? account.host.name)
    /** The server as `share-list-sign-in.ts` takes it. */
    const server = $derived<ShareListServer>({ host, label: hostLabel })

    // Local state
    let shares = $state<ShareInfo[]>([])
    let authMode = $state<AuthMode>('unknown')
    let loading = $state(true)
    let error = $state<ShareListError | null>(null)
    // On `..`, the way a file list opens on its parent row. Landing by name (`landOnShare`) wins.
    let cursorIndex = $state(0)

    // Sorted shares for display (case-insensitive alphabetical)
    const sortedShares = $derived(
        [...shares].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })),
    )
    /** Every row the list shows: `..`, then the shares. */
    const rowCount = $derived(sortedShares.length + PARENT_ROWS)

    /** The share on row `row`, or `null` for `..` and anything out of range. */
    function shareAt(row: number): ShareInfo | null {
        return sortedShares[row - PARENT_ROWS] ?? null
    }

    /** Opens row `row`: `..` goes back to the Servers list, a share mounts. */
    function activateRow(row: number) {
        if (row === 0) {
            onBack?.()
            return
        }
        const share = shareAt(row)
        if (share) void activateShare(share)
    }

    /** The account the list is signed in as, off the listings themselves (`getListedAccount`). */
    const listedAccount = $derived(getListedAccount(host.id))

    // Track authenticated credentials for mounting
    let authenticatedCredentials = $state<ShareListCredentials | null>(null)
    // Forget shows after a guest listing too, once this session read the password (cache only).
    $effect(() => {
        if (!loading) void noteCachedCredentials(host.name).catch(() => {})
    })

    // Container tracking for PageUp/PageDown
    let listContainer: HTMLDivElement | undefined = $state()
    let containerHeight = $state(0)

    // Load shares on mount
    onMount(async () => {
        await loadShares()
    })

    // Another door's listing (the hub's row menu, the palette, the path bar's account) landed
    // for this server: the list follows it, as the header's own buttons make it.
    $effect(() => {
        const state = getShareState(host.id)
        if (state?.status !== 'loaded') return
        untrack(() => {
            if (loading) return
            shares = state.result.shares
            authMode = state.result.authMode
            error = null
        })
    })

    // Sync share list to MCP when shares or cursor change
    $effect(() => {
        dependOn(sortedShares.length, cursorIndex, loading)
        void syncPaneStateToMcp()
    })

    // A queued share (an smb://host/share address, or "Copy path between panes" with the
    // cursor on a share) mounts once the list is on screen. Spending it is the owner's.
    $effect(() => {
        if (!autoMountShare || loading || sortedShares.length === 0) return
        untrack(() => {
            const match = takeAutoMount?.(sortedShares) ?? null
            if (match) void activateShare(match)
        })
    })

    // The share `..` came out of: the cursor lands on it once the list is on screen, once per
    // distinct value. A list that didn't load (an error, a sign-in) keeps it for when it does.
    let lastLanding: string | undefined
    $effect(() => {
        const shareName = landOnShare
        if (!shareName || shareName === lastLanding || loading || error !== null || sortedShares.length === 0) return
        lastLanding = shareName
        const index = indexOfName(sortedShares, shareName)
        if (index >= 0) {
            cursorIndex = index + PARENT_ROWS
            scrollToIndex(cursorIndex)
        }
        onLandedOnShare?.()
    })

    /**
     * Activates a share (Enter, double-click, or auto-mount). When the listing reported
     * `creds_required` but we hold no credentials, route through the login form first
     * instead of firing a doomed guest mount. This combination is real: on macOS the
     * share-listing fallback (`smbutil view -N`) authenticates via the SYSTEM Keychain,
     * which Cmdr can't reuse for mounting, so the list renders fine while
     * `authenticatedCredentials` stays null.
     */
    async function activateShare(share: ShareInfo) {
        // When creds are required but none are in memory, try Cmdr's stored password
        // first. The share list often loads via the system Keychain (smbutil) without
        // ever exercising our own creds, so a working password may already be saved.
        if (authMode === 'creds_required' && !authenticatedCredentials) {
            const stored = await readStoredCredentials(server)
            if (stored) {
                authenticatedCredentials = stored
            }
        }

        // Attempt the mount with whatever credentials we have (possibly none). Two
        // cases this handles without ever pre-prompting:
        //   - The share is already mounted → the backend short-circuits to the existing
        //     mount (no auth), so reaching an already-mounted share just navigates.
        //   - It genuinely needs auth we don't have → the mount fails and
        //     `NetworkMountView` surfaces the login form (its mount-failure handler),
        //     a single login surface with no dead end.
        // Don't pre-prompt here on `creds_required`: that re-prompts for shares that are
        // already mounted (the listing's `creds_required` says nothing about whether
        // THIS share is currently mounted).
        onShareSelect?.(share, authenticatedCredentials)
    }

    /** Sync share list to MCP so agents see the same data as the UI. */
    async function syncPaneStateToMcp() {
        if (!paneId) return

        try {
            const shareEntries: PaneFileEntry[] = sortedShares.map((share) => ({
                name: share.comment ? `${share.name}  comment="${share.comment}"` : share.name,
                path: `smb://${host.ipAddress ?? host.name}/${share.name}`,
                isDirectory: true,
                size: null,
                recursiveSize: null,
                modified: null,
                recursiveSizeUpdating: null,
            }))
            // `..` is a row only while the shares are on screen, so the agent's indices match the person's.
            const files = sortedShares.length > 0 ? [PARENT_MCP_ENTRY, ...shareEntries] : shareEntries

            const state: PaneState = {
                path: `smb://${host.ipAddress ?? host.name}/`,
                volumeId: 'network',
                volumeName: `Network > ${host.name}`,
                files,
                cursorIndex,
                viewMode: 'full',
                selectedIndices: [],
                totalFiles: files.length,
                loadedStart: 0,
                loadedEnd: files.length,
            }

            if (paneId === 'left') {
                await updateLeftPaneState(state)
            } else {
                await updateRightPaneState(state)
            }
        } catch {
            // Silently ignore sync errors
        }
    }

    async function loadShares() {
        loading = true
        error = null

        // Check if we have cached share state
        const cachedState = getShareState(host.id)
        if (cachedState?.status === 'loaded') {
            shares = cachedState.result.shares
            authMode = cachedState.result.authMode
            loading = false
            return
        }
        if (cachedState?.status === 'error') {
            if (isListingAuthError(cachedState.error)) {
                await settleListingAuth(cachedState.error)
                return
            }
            // Non-auth error (host_unreachable, timeout, etc.): the user is
            // explicitly opening the host, so retry. The initial background
            // prefetch may have run before the host was ready.
        }

        // Fetch shares
        try {
            const result = await fetchShares(host)
            shares = result.shares
            authMode = result.authMode
        } catch (e) {
            const shareError = shareListErrorOf(e)
            if (isListingAuthError(shareError)) {
                await settleListingAuth(shareError)
                return
            }
            error = shareError
        } finally {
            loading = false
        }
    }

    /**
     * A listing that needs a sign-in: try the stored credential silently first
     * (the loading indicator stays up, because nothing is being asked yet), and
     * only ask when that doesn't land.
     */
    async function settleListingAuth(shareError: ShareListError) {
        const stored = await tryStoredCredentials(server)
        if (stored) {
            authenticatedCredentials = stored.credentials
            applyAttempt(stored.attempt)
        }
        loading = false
        // Only a listing that actually brought shares counts as settled.
        if (stored && shares.length > 0) return
        error = shareError
        await askForCredentials(refusalForShareError(shareError))
    }

    /** What a listing round came to, on screen. A refused credential changes nothing here. */
    function applyAttempt(attempt: ShareListAttempt | ShareListSheetOutcome) {
        if (attempt.kind === 'listed') {
            shares = attempt.result.shares
            authMode = attempt.result.authMode
            error = null
            authenticatedCredentials = attempt.credentials
        } else if (attempt.kind === 'failed') {
            error = attempt.error
        }
    }

    /**
     * Runs one sheet round and shows what it came to. `null` when another round's sheet is
     * up: the module asks one at a time (`share-list-sign-in.ts`), and while it's up the
     * rows aren't offered past the modal (`isShareListSheetUp`).
     */
    async function withSheet(
        round: () => Promise<ShareListSheetOutcome | null>,
    ): Promise<ShareListSheetOutcome | null> {
        const outcome = await round()
        if (outcome) applyAttempt(outcome)
        return outcome
    }

    /**
     * Hands the listing's credential question to the one sign-in sheet.
     *
     * ❗ Cancelling goes BACK to the host list. This sheet only ever opens
     * because the share LISTING itself needs a sign-in, so "not now" means the
     * user has no business on this host — leaving them on a list they can't read
     * would be an empty room with no exit. (Share-activation auth is the mount's
     * question, and `../pane/share-mount.svelte.ts` asks it.)
     */
    async function askForCredentials(refusal: ConnectRefusalKind) {
        const outcome = await withSheet(() => askToList(server, { refusal, guestAllowed: authMode === 'guest_allowed' }))
        if (outcome?.kind === 'cancelled') onBack?.()
    }

    /** "Sign in as…" in the header. ❗ A cancel stays on the list: the person can already see it. */
    async function signInAs() {
        await withSheet(() => signInAsOnSheet(server, listedAccount))
    }

    /** "Browse as guest": the way back from an account, where a guest listing worked on this server. */
    async function useGuest() {
        applyAttempt(await listAsGuest(server))
    }

    /** Move cursor to a specific row, `..` being row 0 (used by MCP move_cursor tool). */
    export function setCursorIndex(index: number) {
        cursorIndex = Math.max(0, Math.min(index, rowCount - 1))
        scrollToIndex(cursorIndex)
    }

    /** Find a row by name (`..` or a share), returns its index or -1. */
    export function findItemIndex(name: string): number {
        if (name === '..') return 0
        const index = sortedShares.findIndex((s) => s.name.toLowerCase() === name.toLowerCase())
        return index < 0 ? -1 : index + PARENT_ROWS
    }

    /**
     * Type-to-jump: the cursor goes to the first share whose name starts with `prefix`; answers
     * that name, or `null`. ❌ Never `..`: it's no name anyone types.
     */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by NetworkMountView
    export function jumpToPrefix(prefix: string): string | null {
        const index = sortedShares.findIndex((s) => nameStartsWith(s.name, prefix))
        if (index < 0) return null
        setCursorIndex(index + PARENT_ROWS)
        return sortedShares[index].name
    }

    /** The rows on offer (`..` and the shares); `0` while the sign-in sheet is up or the host listed nothing. */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by MCP move_cursor's range check
    export function getItemCount(): number {
        return isShareListSheetUp() || sortedShares.length === 0 ? 0 : rowCount
    }

    /**
     * Returns the share under the cursor, or `null` when nothing valid is highlighted
     * (sign-in sheet up, empty list, out-of-range index). Consumed by the
     * "Copy path between panes" command so cursor-on-share mounts that share on the
     * target pane.
     */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by NetworkMountView
    export function getShareUnderCursor(): ShareInfo | null {
        if (isShareListSheetUp()) return null
        return shareAt(cursorIndex)
    }

    /** Opens the row under the cursor — same action Enter triggers: `..` goes back, a share mounts. */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by NetworkMountView / MCP
    export function openCursorItem(): void {
        activateRow(cursorIndex)
    }

    /** Scrolls to make the cursor visible */
    function scrollToIndex(index: number) {
        if (listContainer) scrollRowIntoView(listContainer, index, SHARE_ROW_HEIGHT, containerHeight)
    }

    /**
     * Escape, Backspace, and `⌘↑` all return to the host list (`share.back`'s
     * registry keys; `⌘↑` mirrors the file list's `⌘↑` = parent). Matched on the
     * whole combo and handled before the arrow handler, which would otherwise treat
     * `⌘↑` as a cursor move.
     */
    function handleBackToHostKey(e: KeyboardEvent): boolean {
        if (eventMatchesCommand(e, 'share.back')) {
            e.preventDefault()
            onBack?.()
            return true
        }
        return false
    }

    /**
     * Handle keyboard navigation. Runs before the document-level dispatcher in
     * `+page.svelte` (bubble-phase on `document`, this pane is a descendant), so a
     * branch acting on a centrally dispatched combo must `stopPropagation()` or the
     * command runs twice. Nothing reads a return value: `preventDefault` +
     * `stopPropagation` is how a branch says it claimed the key.
     */
    export function handleKeyDown(e: KeyboardEvent): void {
        // The sign-in sheet is modal and owns the keyboard while it is up.
        if (isShareListSheetUp()) return

        if (sortedShares.length === 0) return

        // Escape / Backspace / ⌘↑ → back to the host list (before the arrow handler).
        if (handleBackToHostKey(e)) return

        // Try centralized navigation shortcuts first (PageUp, PageDown, Home, End, Option+arrows)
        const visibleItems = Math.max(1, Math.floor(containerHeight / SHARE_ROW_HEIGHT))
        const navResult = handleNavigationShortcut(e, {
            currentIndex: cursorIndex,
            totalCount: rowCount,
            visibleItems,
        })
        if (navResult?.handled) {
            e.preventDefault()
            cursorIndex = navResult.newIndex
            scrollToIndex(cursorIndex)
            return
        }

        // Everything below is an unmodified key: `⌘←` / `⌘→` (copy path between
        // panes) and other modifier supersets belong to the document dispatcher.
        if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return

        const next = cursorAfterArrow(e.key, cursorIndex, rowCount)
        if (next !== null) {
            e.preventDefault()
            cursorIndex = next
            scrollToIndex(cursorIndex)
            return
        }

        // Handle action keys. `stopPropagation` is the claim this branch makes on the
        // key: bare Enter resolves centrally to `nav.open`, whose handler posts Enter
        // back to the focused pane, which hands the network view every key. Without it,
        // one Enter mounted the share twice.
        if (eventMatchesCommand(e, 'share.selectShare')) {
            claimKey(e)
            activateRow(cursorIndex)
        }
    }

    async function handleForgetPassword() {
        if (await forgetPassword(server)) authenticatedCredentials = null
    }

    function handleRetry() {
        error = null
        clearShareState(host.id)
        void loadShares()
    }
</script>

<div class="share-browser" class:is-focused={isFocused}>
    {#if loading}
        <div class="loading-state">
            <Spinner size="md" />
            {tString('fileExplorer.network.share.connecting', { hostName: hostLabel })}
        </div>
    {:else if error && isListingAuthError(error)}
        <!-- ❗ A server that wants an account is a normal step, not a failure: no red
             icon, no "Couldn't connect", and no Retry, which without an account
             gets the same answer (cmdr-reports#10). The sheet sits on top of this. -->
        <div class="sign-in-state">
            <div class="sign-in-icon"><Icon name="lock" size={32} aria-hidden="true" /></div>
            <div class="sign-in-title">
                {tString('fileExplorer.network.share.signInTitle', { hostName: hostLabel })}
            </div>
            <div class="sign-in-message">
                {error.type === 'auth_required'
                    ? tString('fileExplorer.network.share.signInMessage')
                    : renderShareListError(error, hostLabel)}
            </div>
            <div class="error-actions">
                <Button variant="primary" onclick={() => void askForCredentials('needs_credentials')}
                    >{tString('fileExplorer.network.signIn')}</Button
                >
                <Button variant="secondary" onclick={onBack}>{tString('fileExplorer.network.back')}</Button>
            </div>
        </div>
    {:else if error}
        <div class="error-state">
            <div class="error-icon"><Icon name="circle-alert" size={32} aria-hidden="true" /></div>
            <div class="error-title">
                {tString('fileExplorer.network.share.connectFailedTitle', { hostName: hostLabel })}
            </div>
            <div class="error-message">{renderShareListError(error, hostLabel)}</div>
            {#if error.type === 'missing_dependency' && error.installCommand}
                <CopyBox text={error.installCommand} />
                <div class="error-actions">
                    <Button variant="secondary" onclick={handleRetry}>{tString('fileExplorer.network.retry')}</Button>
                    <Button variant="secondary" onclick={onBack}>{tString('fileExplorer.network.back')}</Button>
                </div>
            {:else}
                <div class="error-actions">
                    <Button variant="secondary" onclick={handleRetry}>{tString('fileExplorer.network.retry')}</Button>
                    <Button variant="secondary" onclick={() => void askForCredentials('needs_credentials')}
                        >{tString('fileExplorer.network.signIn')}</Button
                    >
                    <Button variant="secondary" onclick={onBack}>{tString('fileExplorer.network.back')}</Button>
                </div>
            {/if}
        </div>
    {:else if sortedShares.length === 0}
        <div class="empty-state">
            <div class="empty-icon"><Icon name="folder" size={32} aria-hidden="true" /></div>
            <div class="empty-title">{tString('fileExplorer.network.share.noSharesTitle')}</div>
            <div class="empty-message">{tString('fileExplorer.network.share.noSharesMessage')}</div>
            <div class="error-actions">
                <Button variant="secondary" onclick={() => void askForCredentials('needs_credentials')}
                    >{tString('fileExplorer.network.signIn')}</Button
                >
                <Button variant="secondary" onclick={onBack}>{tString('fileExplorer.network.back')}</Button>
            </div>
        </div>
    {:else}
        <PlacesHeader
            {hostLabel}
            account={listedAccount}
            canForgetPassword={authenticatedCredentials !== null || getCredentialStatus(host.name) === 'has_creds'}
            shareCount={sortedShares.length}
            {onBack}
            onSignInAs={() => void signInAs()}
            onUseGuest={listedAccount?.kind === 'user' && guestListingWorked(host.id) ? () => void useGuest() : undefined}
            onForgetPassword={() => void handleForgetPassword()}
        />
        <div class="share-list" role="list" bind:this={listContainer} bind:clientHeight={containerHeight}>
            <!-- `..`: back to the Servers list, the file list's parent row in look and meaning. -->
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <div
                class="share-row"
                data-parent-row
                class:is-under-cursor={cursorIndex === 0}
                class:is-focused-and-under-cursor={isFocused && cursorIndex === 0}
                role="listitem"
                aria-label={tString('fileExplorer.network.share.parentRowName')}
                onclick={() => {
                    cursorIndex = 0
                }}
                ondblclick={() => {
                    activateRow(0)
                }}
                onkeydown={() => {}}
            >
                <span class="share-icon"><Icon name="folder" size={16} aria-hidden="true" /></span>
                <span class="share-name">..</span>
            </div>
            {#each sortedShares as share, index (share.name)}
                {@const row = index + PARENT_ROWS}
                <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                <div
                    class="share-row"
                    class:is-under-cursor={row === cursorIndex}
                    class:is-focused-and-under-cursor={isFocused && row === cursorIndex}
                    role="listitem"
                    onclick={() => {
                        cursorIndex = row
                    }}
                    ondblclick={() => {
                        activateRow(row)
                    }}
                    onkeydown={() => {}}
                >
                    <span class="share-icon"><Icon name="folder" size={16} aria-hidden="true" /></span>
                    <span class="share-name">{share.name}</span>
                    {#if share.comment}
                        <span class="share-comment">{share.comment}</span>
                    {/if}
                </div>
            {/each}
        </div>
    {/if}
</div>

<style>
    .share-browser {
        display: flex;
        flex-direction: column;
        height: 100%;
        font-size: var(--font-size-sm);
        font-family: var(--font-system), sans-serif;
    }

    .loading-state,
    .error-state,
    .sign-in-state,
    .empty-state {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        height: 100%;
        padding: var(--spacing-xl);
        gap: var(--spacing-md);
        color: var(--color-text-secondary);
    }

    .error-icon,
    .sign-in-icon,
    .empty-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
    }
    .error-icon {
        color: var(--color-error);
    }
    .sign-in-icon,
    .empty-icon {
        color: var(--color-text-tertiary);
    }

    .error-title,
    .sign-in-title,
    .empty-title {
        font-size: var(--font-size-lg);
        font-weight: 500;
        color: var(--color-text-primary);
    }

    .error-message,
    .sign-in-message,
    .empty-message {
        font-size: var(--font-size-sm);
        color: var(--color-text-tertiary);
        text-align: center;
    }

    .error-actions {
        display: flex;
        gap: var(--spacing-sm);
        margin-top: var(--spacing-sm);
    }

    .share-list {
        flex: 1;
        overflow-y: auto;
    }

    .share-row {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
        height: 20px;
        padding: var(--spacing-xxs) var(--spacing-sm);
        cursor: default;
    }

    .share-row.is-under-cursor {
        background-color: var(--color-cursor-inactive);
    }

    .share-row.is-focused-and-under-cursor {
        background-color: var(--color-cursor-active);
    }

    .share-icon {
        display: inline-flex;
        align-items: center;
        color: var(--color-text-secondary);
    }

    .share-name {
        font-weight: 500;
    }

    .share-comment {
        color: var(--color-text-tertiary);
        margin-left: auto;
        font-size: var(--font-size-sm);
    }

    .share-row.is-focused-and-under-cursor .share-comment {
        color: var(--color-text-secondary);
    }
</style>
