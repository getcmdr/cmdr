<script lang="ts">
    /**
     * The places under one account: an SMB host's shares today, a storage
     * account's buckets later. Raises the sign-in form when the listing needs one.
     */
    import { onMount } from 'svelte'
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
        setShareState,
        setCredentialStatus,
        getCredentialStatus,
        noteCachedCredentials,
        forgetCredentials,
        getListedAccount,
        setListedAccount,
        guestListingWorked,
    } from './network-store.svelte'
    import PlacesHeader from './PlacesHeader.svelte'
    import {
        listSharesWithCredentials,
        saveSmbCredentials,
        getSmbCredentials,
        isUsingCredentialFileFallback,
        updateKnownShare,
        setSmbAccountPreference,
    } from '#lib/tauri-commands/index.js'
    import { addToast } from '#lib/ui/toast/index.js'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { getNetworkTimeoutMs, getShareCacheTtlMs } from '#lib/settings/network-settings.js'
    import { isListingAuthError, openSmbSignInSheet, refusalForShareError } from './smb-sign-in'
    import type { ConnectRefusalKind } from '#lib/servers/connect-refusals.js'
    import type { SignInAttemptOutcome } from '#lib/servers/sign-in-contract.js'
    import { handleNavigationShortcut } from '../navigation/keyboard-shortcuts'
    import { eventMatchesCommand } from '#lib/shortcuts/index.js'
    import { claimKey } from '#lib/shortcuts/claim-key.js'
    import { cursorAfterArrow, indexOfName, nameStartsWith, scrollRowIntoView } from './list-cursor'
    import { updateLeftPaneState, updateRightPaneState, type PaneState, type PaneFileEntry } from '#lib/tauri-commands/index.js'

    async function notifyIfUsingFileFallback(): Promise<void> {
        if (await isUsingCredentialFileFallback()) {
            addToast(tString('fileExplorer.network.share.credentialsStoredLocally'), {
                level: 'info',
                id: 'credential-file-fallback',
            })
        }
    }

    /** Row height for share list (matches Full list) */
    const SHARE_ROW_HEIGHT = 20

    interface Props {
        /** Whose places these are. One SMB arm today; see `PlacesAccount`. */
        account: PlacesAccount
        /** Which pane this browser lives in (for MCP state sync) */
        paneId?: 'left' | 'right'
        /** Whether this pane is focused */
        isFocused?: boolean
        /** Auto-mount this share name after loading (from smb://host/share URL) */
        autoMountShare?: string
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

    // Local state
    let shares = $state<ShareInfo[]>([])
    let authMode = $state<AuthMode>('unknown')
    let loading = $state(true)
    let error = $state<ShareListError | null>(null)
    let cursorIndex = $state(0)

    // Sorted shares for display (case-insensitive alphabetical)
    const sortedShares = $derived(
        [...shares].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })),
    )

    /**
     * True while the sign-in sheet is up for this host. The pane behind it keeps
     * whatever it was showing; this only stops the cursor exports from offering
     * rows nobody can reach past a modal.
     */
    let signingIn = $state(false)

    /** The account the list is signed in as, off the listings themselves (`getListedAccount`). */
    const listedAccount = $derived(getListedAccount(host.id))

    // Track authenticated credentials for mounting
    let authenticatedCredentials = $state<{ username: string; password: string } | null>(null)
    // Forget shows after a guest listing too, once this session read the password (cache only).
    $effect(() => {
        if (!loading) void noteCachedCredentials(host.name).catch(() => {})
    })

    // Auto-mount tracking: track the last share we tried so the same prop value
    // doesn't re-fire, but a new value (for example via "Copy path between panes"
    // with cursor on a different share) does.
    let lastAutoMountAttempt = $state<string | undefined>(undefined)

    // Container tracking for PageUp/PageDown
    let listContainer: HTMLDivElement | undefined = $state()
    let containerHeight = $state(0)

    // Load shares on mount
    onMount(async () => {
        await loadShares()
    })

    // Sync share list to MCP when shares or cursor change
    $effect(() => {
        dependOn(sortedShares.length, cursorIndex, loading)
        void syncPaneStateToMcp()
    })

    // Auto-mount a share if requested (from smb://host/share URL, or "Copy path
    // between panes" with cursor on a share). Fires once per distinct prop value.
    $effect(() => {
        const shareName = autoMountShare
        if (!shareName || shareName === lastAutoMountAttempt || loading || sortedShares.length === 0) return
        lastAutoMountAttempt = shareName

        const match = sortedShares.find(
            (s) => s.name.localeCompare(shareName, undefined, { sensitivity: 'base' }) === 0,
        )
        if (match) {
            void activateShare(match)
        } else {
            addToast(tString('fileExplorer.network.share.notFound', { shareName, hostName: hostLabel }), {
                level: 'warn',
            })
        }
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
            cursorIndex = index
            scrollToIndex(index)
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
            const stored = await loadStoredCredentials()
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

    /** Reads Cmdr's stored credentials for this host, or null if none are saved. */
    async function loadStoredCredentials(): Promise<{ username: string; password: string } | null> {
        try {
            const creds = await getSmbCredentials(host.name, null)
            return { username: creds.username, password: creds.password }
        } catch {
            return null
        }
    }

    /** Sync share list to MCP so agents see the same data as the UI. */
    async function syncPaneStateToMcp() {
        if (!paneId) return

        try {
            const files: PaneFileEntry[] = sortedShares.map((share) => ({
                name: share.comment ? `${share.name}  comment="${share.comment}"` : share.name,
                path: `smb://${host.ipAddress ?? host.name}/${share.name}`,
                isDirectory: true,
                size: null,
                recursiveSize: null,
                modified: null,
                recursiveSizeUpdating: null,
            }))

            const state: PaneState = {
                path: `smb://${host.ipAddress ?? host.name}/`,
                volumeId: 'network',
                volumeName: `Network > ${host.name}`,
                files,
                cursorIndex,
                viewMode: 'full',
                selectedIndices: [],
                totalFiles: sortedShares.length,
                loadedStart: 0,
                loadedEnd: sortedShares.length,
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
        const stored = await tryStoredCredentials()
        loading = false
        if (stored) return
        error = shareError
        await askForCredentials(refusalForShareError(shareError))
    }

    /**
     * Hands the listing's credential question to the one sign-in sheet.
     *
     * ❗ Cancelling goes BACK to the host list. This sheet only ever opens
     * because the share LISTING itself needs a sign-in, so "not now" means the
     * user has no business on this host — leaving them on a list they can't read
     * would be an empty room with no exit. (Share-activation auth is the mount's
     * question, and `NetworkMountView` asks it.)
     */
    async function askForCredentials(refusal: ConnectRefusalKind) {
        // ❗ One ask at a time. A second `openSignInSheet` closes the first as
        // `cancelled`, and this caller reads that as "the user said no" and walks
        // back to the host list from under a sheet that is still up.
        if (signingIn) return
        signingIn = true
        try {
            const result = await openSmbSignInSheet({
                host,
                label: hostLabel,
                guestAllowed: authMode === 'guest_allowed',
                refusal,
                attempt: (answer) => listWithCredentials(answer.username, answer.password, answer.remember, 'typed'),
            })
            if (result.kind === 'cancelled') onBack?.()
        } finally {
            signingIn = false
        }
    }

    /**
     * "Sign in as…" in the header: the same sheet, asked by the person rather than a
     * refusal, so it opens with no refusal sentence, prefilled with the account the
     * list is signed in as, and ❗ a cancel stays on the list (they can already see it).
     * No guest choice: the sheet selects guest whenever it offers it, and this button
     * is for signing in with an account.
     */
    async function signInAs() {
        if (signingIn) return
        signingIn = true
        try {
            await openSmbSignInSheet({
                host,
                label: hostLabel,
                guestAllowed: false,
                initialUsername: listedAccount?.kind === 'user' ? listedAccount.username : undefined,
                attempt: (answer) => listWithCredentials(answer.username, answer.password, answer.remember, 'typed'),
            })
        } finally {
            signingIn = false
        }
    }

    /** "Browse as guest": the way back from an account, where a guest listing worked on this server. */
    async function useGuest() {
        // ❗ The account goes FIRST: while it's set, the backend never lists this server as guest.
        await setSmbAccountPreference(host.name, null).catch(() => false)
        await listWithCredentials(null, null, false, 'typed')
    }

    /** Try to use stored credentials. Returns true if shares were loaded. */
    async function tryStoredCredentials(): Promise<boolean> {
        const serverName = host.name

        // Try to get credentials directly - don't check hasSmbCredentials first
        // as that causes an extra Keychain dialog (each Keychain access = 1 dialog)
        try {
            const creds = await getSmbCredentials(serverName, null)
            // Store credentials in memory for mounting later
            authenticatedCredentials = { username: creds.username, password: creds.password }
            await listWithCredentials(creds.username, creds.password, false, 'saved')
            // `listWithCredentials` never throws; it answers an outcome. Only
            // report success if shares were actually loaded.
            return shares.length > 0
        } catch {
            // No stored credentials or retrieval failed
            return false
        }
    }

    /**
     * One listing round-trip with what the user offered.
     *
     * ❗ Answers `handed_off` on success: nothing here connects a VOLUME, so the
     * sheet has nothing more to say and the share list it just loaded is the
     * result. A refusal about the credential keeps the sheet open; anything else
     * closes it onto the pane's own error state, which has the retry.
     */
    async function listWithCredentials(
        username: string | null,
        password: string | null,
        rememberInKeychain: boolean,
        credentialSource: 'typed' | 'saved',
    ): Promise<SignInAttemptOutcome> {
        try {
            // Clear cached state to force refetch
            clearShareState(host.id)

            const result = await listSharesWithCredentials(
                host.id,
                host.hostname ?? host.name,
                host.ipAddress,
                host.port,
                username,
                password,
                credentialSource,
                getNetworkTimeoutMs(),
                getShareCacheTtlMs(),
            )

            shares = result.shares
            authMode = result.authMode
            error = null

            // Update global share state so ServersHub shows correct info
            setShareState(host.id, result)

            // Update credential status, and the account the header names
            setCredentialStatus(host.name, username ? 'has_creds' : 'no_creds')
            setListedAccount(host.id, username !== null ? { kind: 'user', username } : { kind: 'guest' })
            // ❗ The choice sticks: it becomes the server's account (the typed-username preference),
            // so later listings, the background ones included, sign in as it instead of as guest.
            void setSmbAccountPreference(host.name, username).catch(() => {})

            // Store credentials for mounting (empty password is valid for SMB)
            if (username !== null) {
                authenticatedCredentials = { username, password: password ?? '' }
            } else {
                authenticatedCredentials = null
            }

            // Save credentials to Keychain if requested
            if (rememberInKeychain && username !== null && password !== null) {
                await saveSmbCredentials(host.name, null, username, password)
                await notifyIfUsingFileFallback()
            }

            // Update known shares store
            await updateKnownShare(
                host.name,
                '', // Server-level, not share-specific
                username ? 'credentials' : 'guest',
                authMode === 'guest_allowed' ? 'guest_or_credentials' : 'credentials_only',
                username,
            )
            return { kind: 'handed_off' }
        } catch (e) {
            const shareError = shareListErrorOf(e)
            if (shareError.type === 'auth_failed') {
                // Mark credentials as failed
                setCredentialStatus(host.name, 'failed')
            }
            const refusal = refusalForShareError(shareError)
            if (refusal === 'authentication_rejected' || refusal === 'needs_credentials') {
                return { kind: 'refused', refusal }
            }
            error = shareError
            return { kind: 'handed_off' }
        }
    }

    /** Move cursor to a specific index (used by MCP move_cursor tool). */
    export function setCursorIndex(index: number) {
        cursorIndex = Math.max(0, Math.min(index, sortedShares.length - 1))
        scrollToIndex(cursorIndex)
    }

    /** Find a share by name, returns its index or -1. */
    export function findItemIndex(name: string): number {
        return sortedShares.findIndex((s) => s.name.toLowerCase() === name.toLowerCase())
    }

    /** Type-to-jump: the cursor goes to the first share whose name starts with `prefix`; answers that name, or `null`. */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by NetworkMountView
    export function jumpToPrefix(prefix: string): string | null {
        const index = sortedShares.findIndex((s) => nameStartsWith(s.name, prefix))
        if (index < 0) return null
        setCursorIndex(index)
        return sortedShares[index].name
    }

    /** The shares on offer; `0` while the sign-in sheet is up or the host listed nothing. */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by MCP move_cursor's range check
    export function getItemCount(): number {
        return signingIn ? 0 : sortedShares.length
    }

    /**
     * Returns the share under the cursor, or `null` when nothing valid is highlighted
     * (sign-in sheet up, empty list, out-of-range index). Consumed by the
     * "Copy path between panes" command so cursor-on-share mounts that share on the
     * target pane.
     */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by NetworkMountView
    export function getShareUnderCursor(): ShareInfo | null {
        if (signingIn) return null
        if (cursorIndex < 0 || cursorIndex >= sortedShares.length) return null
        return sortedShares[cursorIndex]
    }

    /** Opens the share under the cursor — same action Enter triggers. */
    // noinspection JSUnusedGlobalSymbols -- used dynamically by NetworkMountView / MCP
    export function openCursorItem(): void {
        if (cursorIndex >= 0 && cursorIndex < sortedShares.length) {
            void activateShare(sortedShares[cursorIndex])
        }
    }

    function handleShareClick(index: number) {
        cursorIndex = index
    }

    function handleShareDoubleClick(index: number) {
        if (index >= 0 && index < sortedShares.length) {
            void activateShare(sortedShares[index])
        }
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
        if (signingIn) return

        if (sortedShares.length === 0) return

        // Escape / Backspace / ⌘↑ → back to the host list (before the arrow handler).
        if (handleBackToHostKey(e)) return

        // Try centralized navigation shortcuts first (PageUp, PageDown, Home, End, Option+arrows)
        const visibleItems = Math.max(1, Math.floor(containerHeight / SHARE_ROW_HEIGHT))
        const navResult = handleNavigationShortcut(e, {
            currentIndex: cursorIndex,
            totalCount: sortedShares.length,
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

        const next = cursorAfterArrow(e.key, cursorIndex, sortedShares.length)
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
            if (cursorIndex >= 0 && cursorIndex < sortedShares.length) {
                void activateShare(sortedShares[cursorIndex])
            }
        }
    }

    async function handleForgetPassword() {
        try {
            await forgetCredentials(host.name)
            authenticatedCredentials = null
            addToast(tString('fileExplorer.network.forgotPassword', { hostName: hostLabel }), { level: 'success' })
        } catch {
            addToast(tString('fileExplorer.network.deletePasswordFailed'), { level: 'error' })
        }
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
            {#each sortedShares as share, index (share.name)}
                <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                <div
                    class="share-row"
                    class:is-under-cursor={index === cursorIndex}
                    class:is-focused-and-under-cursor={isFocused && index === cursorIndex}
                    role="listitem"
                    onclick={() => {
                        handleShareClick(index)
                    }}
                    ondblclick={() => {
                        handleShareDoubleClick(index)
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
