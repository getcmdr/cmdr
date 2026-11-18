<script lang="ts">
    import { onMount, tick } from 'svelte'
    import {
        verifyLicense,
        commitLicense,
        validateLicenseWithServer,
        getLicenseInfo,
        resetLicense,
        openExternalUrl,
        parseActivationError,
        type LicenseInfo,
        type LicenseStatus,
    } from '#lib/tauri-commands/index.js'
    import {
        loadLicenseStatus,
        getCachedStatus,
        setCachedStatus,
        isPendingVerification,
        setPendingVerification,
    } from './licensing-store.svelte'
    import ModalDialog from '#lib/ui/ModalDialog.svelte'
    import Button from '#lib/ui/Button.svelte'
    import LinkButton from '#lib/ui/LinkButton.svelte'
    import TextInput from '#lib/ui/TextInput.svelte'
    import Spinner from '#lib/ui/Spinner.svelte'
    import Trans from '#lib/intl/Trans.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { formatLicenseDate } from '#lib/settings/sections/license-section-utils.js'
    import { addToast } from '#lib/ui/toast/toast-store.svelte.js'

    interface Props {
        onClose: () => void
        onSuccess: () => void
    }

    const { onClose, onSuccess }: Props = $props()

    let licenseKey = $state('')
    let error = $state('')
    let isActivating = $state(false)
    let hasError = $state(false)
    let inputElement: HTMLInputElement | undefined = $state()

    let existingLicense = $state<LicenseInfo | null>(null)
    let isLoading = $state(true)
    let isConfirmingReset = $state(false)
    let isServerInvalidError = $state(false)
    let serverInvalidRetryCount = $state(0)

    let errorHelpHint = $state('')
    /** Bumped by every `showError`, so a retry failing with the same words re-renders them and the alert speaks again. */
    let errorSeq = $state(0)
    const uid = $props.id()
    /** The error line: the key field's description, inside the always-mounted alert region. */
    const errorId = `${uid}-error`

    function showError(message: string, hint = ''): void {
        error = message
        errorHelpHint = hint
        hasError = true
        errorSeq++
    }

    function clearError(): void {
        error = ''
        errorHelpHint = ''
        hasError = false
        isServerInvalidError = false
    }

    const SUPPORT_EMAIL = 'hello@getcmdr.com'

    const status = getCachedStatus()

    const hasExistingLicense = $derived(existingLicense !== null)
    const cleanedKey = $derived(licenseKey.replace(/\s/g, ''))


    function getLicenseTypeLabel(licenseType: string | null | undefined): string | null {
        if (licenseType === 'commercial_perpetual') return tString('licensing.dialog.typeCommercialPerpetual')
        if (licenseType === 'commercial_subscription') return tString('licensing.dialog.typeCommercialSubscription')
        return null
    }

    // Matches backend VALIDATION_INTERVAL_SECS (7 days) in app_status.rs
    const validationIntervalDays = 7

    function getValidityText(s: LicenseStatus | null, pending: boolean): string | null {
        if (pending && s?.type === 'commercial') {
            return tString('licensing.dialog.validityNotYetVerified')
        }
        if (s?.type === 'commercial') {
            if (s.licenseType === 'commercial_perpetual') {
                return s.expiresAt
                    ? tString('licensing.dialog.validityPerpetualUntil', { date: formatLicenseDate(s.expiresAt) })
                    : tString('licensing.dialog.validityPerpetual')
            }
            return s.expiresAt
                ? tString('licensing.dialog.validityValidUntil', { date: formatLicenseDate(s.expiresAt) })
                : tString('licensing.dialog.validityActive')
        }
        if (s?.type === 'expired') return tString('licensing.dialog.validityExpiredOn', { date: formatLicenseDate(s.expiredAt) })
        return null
    }

    const licenseTypeLabel = $derived(getLicenseTypeLabel(existingLicense?.licenseType))
    const pendingVerification = $derived(isPendingVerification())
    const validityText = $derived(getValidityText(status, pendingVerification))
    const orgName = $derived(existingLicense?.organizationName ?? null)
    const shortCode = $derived(existingLicense?.shortCode ?? null)
    const isServerInvalid = $derived(existingLicense !== null && status?.type === 'personal')

    function getFriendlyError(e: unknown): { error: string; hint: string } {
        const parsed = parseActivationError(e)
        switch (parsed?.code) {
            case 'badSignature':
                return {
                    error: tString('licensing.error.badSignature'),
                    hint: tString('licensing.error.badSignatureHint'),
                }
            case 'invalidFormat':
            case 'badEncoding':
            case 'badPayload':
                return {
                    error: tString('licensing.error.badFormat'),
                    hint: tString('licensing.error.badFormatHint'),
                }
            case 'shortCodeNotFound':
                return {
                    error: tString('licensing.error.shortCodeNotFound'),
                    hint: tString('licensing.error.shortCodeNotFoundHint'),
                }
            case 'networkError':
                return {
                    error: tString('licensing.error.network'),
                    hint: tString('licensing.error.networkHint'),
                }
            case 'serverError':
                return {
                    error: tString('licensing.error.server'),
                    hint: tString('licensing.error.serverHint'),
                }
            case 'serverBusy':
                return {
                    error: tString('licensing.error.serverBusy'),
                    hint: tString('licensing.error.serverHint'),
                }
            default:
                return {
                    error: tString('licensing.error.generic'),
                    hint: tString('licensing.error.genericHint'),
                }
        }
    }

    onMount(async () => {
        try {
            existingLicense = await getLicenseInfo()
        } catch {
            // No existing license
        } finally {
            isLoading = false
        }

        if (!existingLicense) {
            await tick()
            inputElement?.focus()
        }
    })

    function showActivationToast(info: LicenseInfo): void {
        const name = info.organizationName
        const message = name
            ? tString('licensing.dialog.activatedToastNamed', { org: name })
            : tString('licensing.dialog.activatedToast')
        addToast(message, { level: 'success' })
    }

    function buildFallbackStatus(info: LicenseInfo): LicenseStatus | null {
        if (info.licenseType === 'commercial_subscription' || info.licenseType === 'commercial_perpetual') {
            return {
                type: 'commercial',
                licenseType: info.licenseType,
                organizationName: info.organizationName,
                expiresAt: info.expiresAt,
            }
        }
        return null
    }

    function handleNetworkFallback(licenseInfo: LicenseInfo): void {
        const fallbackStatus = buildFallbackStatus(licenseInfo)
        if (fallbackStatus) {
            setCachedStatus(fallbackStatus)
            setPendingVerification(true)
            clearError()
            showActivationToast(licenseInfo)
            onSuccess()
        } else {
            showError(tString('licensing.dialog.networkFallbackError'))
        }
    }

    function buildMailtoUrl(key: string): string {
        const subject = encodeURIComponent(tString('licensing.dialog.mailtoSubject'))
        const body = encodeURIComponent(tString('licensing.dialog.mailtoBody', { key }))
        return `mailto:${SUPPORT_EMAIL}?subject=${subject}&body=${body}`
    }

    async function handleEmailClick(event: MouseEvent) {
        event.preventDefault()
        const keyForEmail = cleanedKey || shortCode || ''
        const mailtoUrl = buildMailtoUrl(keyForEmail)
        await openExternalUrl(mailtoUrl)
    }

    async function handleActivate() {
        if (!cleanedKey) {
            showError(tString('licensing.dialog.emptyKeyError'))
            return
        }

        isActivating = true
        // Don't clear error/hasError here. Keep old error visible during retry to avoid flicker.
        // Each outcome path below sets its own error state.

        try {
            // Step 1: Verify the key offline (Ed25519 + short code exchange if needed).
            // Nothing is written to disk. If this fails, we show a clean error.
            const verifyResult = await verifyLicense(cleanedKey)

            // Step 2: Validate with the license server, passing transactionId explicitly
            // since the key isn't stored yet.
            let newStatus: LicenseStatus | null = null
            try {
                newStatus = await validateLicenseWithServer(verifyResult.info.transactionId)
            } catch {
                // Network error; fall through to network fallback
            }

            // Step 3: Decide whether to commit (persist) the key based on server response.
            if (newStatus?.type === 'commercial') {
                await commitLicense(verifyResult.fullKey, verifyResult.shortCode)
                setPendingVerification(false)
                setCachedStatus(newStatus)
                serverInvalidRetryCount = 0
                clearError()
                showActivationToast(verifyResult.info)
                onSuccess()
            } else if (newStatus?.type === 'expired') {
                // Key IS valid, just expired. Commit it so the user can renew
                await commitLicense(verifyResult.fullKey, verifyResult.shortCode)
                setCachedStatus(newStatus)
                isServerInvalidError = false
                showError(
                    tString('licensing.dialog.expiredOnError', { date: formatLicenseDate(newStatus.expiredAt) }),
                    tString('licensing.dialog.expiredOnHelp'),
                )
            } else if (newStatus?.type === 'personal') {
                // Server checked with Paddle and this transaction is unknown.
                // DON'T commit. Nothing is stored, no cleanup needed.
                serverInvalidRetryCount++
                isServerInvalidError = true
                showError(tString('licensing.dialog.serverInvalidError'))
            } else {
                // Network error (newStatus is null): key is crypto-valid, commit optimistically
                await commitLicense(verifyResult.fullKey, verifyResult.shortCode)
                handleNetworkFallback(verifyResult.info)
            }
        } catch (e) {
            const friendly = getFriendlyError(e)
            isServerInvalidError = false
            showError(friendly.error, friendly.hint)
        } finally {
            isActivating = false
        }
    }

    async function handleResetConfirm() {
        await resetLicense()
        await loadLicenseStatus()
        existingLicense = null
        licenseKey = ''
        isConfirmingReset = false
        await tick()
        inputElement?.focus()
    }

    function handleCancelAfterInvalid() {
        // Invalid keys are never stored, so no cleanup is needed. Just close.
        onClose()
    }

    function handleClose() {
        // Invalid keys are never stored (verify/commit split), so no cleanup needed on close.
        onClose()
    }

    function handleKeydown(event: KeyboardEvent) {
        if (event.key === 'Enter' && !isActivating && !hasExistingLicense) {
            // Claimed, so `ModalDialog` doesn't press the default button too.
            event.preventDefault()
            void handleActivate()
        }
    }

    function handleInputKeydown(event: KeyboardEvent) {
        if (event.key === 'Enter' && !isActivating && !hasExistingLicense) {
            event.preventDefault()
            event.stopPropagation()
            void handleActivate()
        }
    }
</script>

{#snippet email(children: import('svelte').Snippet)}
    <LinkButton href="mailto:{SUPPORT_EMAIL}" onclick={handleEmailClick}>{@render children()}</LinkButton>
{/snippet}

{#snippet getLicense(children: import('svelte').Snippet)}
    <LinkButton
        href="https://getcmdr.com/pricing"
        onclick={(event: MouseEvent) => {
            event.preventDefault()
            void openExternalUrl('https://getcmdr.com/pricing')
        }}>{@render children()}</LinkButton
    >
{/snippet}

<!-- One footer for the three views; none while the license loads, so no empty row shows. -->
{#snippet actions()}
    {#if hasExistingLicense && !isConfirmingReset}
        <Button variant="secondary" onclick={() => (isConfirmingReset = true)}
            >{tString('licensing.dialog.useDifferentKey')}</Button
        >
        <Button variant="primary" onclick={onClose}>{tString('licensing.dialog.close')}</Button>
    {:else if isConfirmingReset}
        <Button variant="secondary" onclick={() => (isConfirmingReset = false)}
            >{tString('licensing.dialog.cancel')}</Button
        >
        <Button variant="primary" onclick={handleResetConfirm}>{tString('licensing.dialog.continue')}</Button>
    {:else if isServerInvalidError}
        <Button variant="secondary" onclick={handleCancelAfterInvalid}
            >{tString('licensing.dialog.cancelButton')}</Button
        >
        <Button variant="primary" isDefault onclick={handleActivate} disabled={isActivating}>
            {#if isActivating}<Spinner size="sm" />{/if}
            {isActivating ? tString('licensing.dialog.checking') : tString('licensing.dialog.tryAgain')}
        </Button>
    {:else}
        <Button variant="secondary" onclick={handleClose}>{tString('licensing.dialog.cancelButton')}</Button>
        <Button variant="primary" isDefault onclick={handleActivate} disabled={isActivating || !cleanedKey}>
            {#if isActivating}<Spinner size="sm" />{/if}
            {isActivating
                ? tString('licensing.dialog.activating')
                : hasError
                  ? tString('licensing.dialog.tryAgain')
                  : tString('licensing.dialog.activate')}
        </Button>
    {/if}
{/snippet}

<ModalDialog
    titleId="dialog-title"
    onkeydown={handleKeydown}
    blur
    dialogId="license"
    onclose={handleClose}
    containerStyle="min-width: 400px; max-width: 500px"
    footer={isLoading ? undefined : actions}
>
    {#snippet title()}
        {#if isLoading}
            {tString('licensing.dialog.loading')}
        {:else if hasExistingLicense}
            {tString('licensing.dialog.detailsTitle')}
        {:else}
            {tString('licensing.dialog.enterTitle')}
        {/if}
    {/snippet}

    <div class="dialog-body">
        {#if !isLoading && hasExistingLicense && !isConfirmingReset}
            {#if isServerInvalid}
                <div class="warning-banner">
                    <span class="warning-text">
                        <Trans
                            key="licensing.dialog.serverInvalidBanner"
                            params={{ email: SUPPORT_EMAIL }}
                            snippets={{ supportEmail: email }}
                        />
                    </span>
                </div>
            {/if}

            <div class="info-box">
                {#if licenseTypeLabel}
                    <div class="info-row">
                        <span class="info-label">{tString('licensing.dialog.labelType')}</span>
                        <span class="info-value">{licenseTypeLabel}</span>
                    </div>
                {/if}
                {#if orgName}
                    <div class="info-row">
                        <span class="info-label">{tString('licensing.dialog.labelOrganization')}</span>
                        <span class="info-value">{orgName}</span>
                    </div>
                {/if}
                {#if validityText}
                    <div class="info-row">
                        <span class="info-label">{tString('licensing.dialog.labelValidity')}</span>
                        <span
                            class="info-value validity-value"
                            class:validity-pending={pendingVerification}
                            class:validity-expired={status?.type === 'expired'}>{validityText}</span
                        >
                    </div>
                    {#if pendingVerification}
                        <div class="info-row-sub">
                            <span class="info-hint">
                                {tString('licensing.dialog.pendingHint', { days: validationIntervalDays })}
                            </span>
                        </div>
                    {/if}
                {/if}
                {#if shortCode}
                    <div class="info-row">
                        <span class="info-label">{tString('licensing.dialog.labelKey')}</span>
                        <span class="info-value mono">{shortCode}</span>
                    </div>
                {/if}
            </div>
        {:else if !isLoading && isConfirmingReset}
            <p class="description">{tString('licensing.dialog.resetConfirm')}</p>
        {:else if !isLoading}
            <p class="description">
                <Trans key="licensing.dialog.enterPrompt" snippets={{ getLicense }} />
            </p>

            <div class="input-group">
                <TextInput
                    bind:inputElement
                    bind:value={licenseKey}
                    invalid={!!error}
                    aria-describedby={error ? errorId : undefined}
                    ariaLabel={tString('licensing.dialog.inputPlaceholder')}
                    placeholder={tString('licensing.dialog.inputPlaceholder')}
                    spellcheck={false}
                    autocomplete="off"
                    autocapitalize="off"
                    disabled={isActivating}
                    onkeydown={handleInputKeydown}
                />
            </div>

            <!-- ❗ Mounted with the field and empty: an alert inserted together with its text may
                 go unspoken. Each failure re-keys its lines, so the same error twice speaks twice. -->
            <div role="alert">
                {#key errorSeq}
                    {#if error}
                        <p class="error-message" id={errorId}>{error}</p>
                        {#if errorHelpHint}
                            <p class="help-text">{errorHelpHint}</p>
                        {/if}
                    {/if}
                {/key}
            </div>
            {#if error}
                <p class="help-text">
                    {#if isServerInvalidError && serverInvalidRetryCount >= 3}
                        <Trans
                            key="licensing.dialog.retryExhausted"
                            params={{ count: serverInvalidRetryCount, email: SUPPORT_EMAIL }}
                            snippets={{ supportEmail: email }}
                        />
                    {:else if isServerInvalidError}
                        <Trans
                            key="licensing.dialog.serverInvalidHelp"
                            params={{ email: SUPPORT_EMAIL }}
                            snippets={{ supportEmail: email }}
                        />
                    {:else}
                        <Trans
                            key="licensing.dialog.genericHelp"
                            params={{ email: SUPPORT_EMAIL }}
                            snippets={{ supportEmail: email }}
                        />
                    {/if}
                </p>
            {/if}
        {/if}
    </div>
</ModalDialog>

<style>
    .description {
        margin: 0 0 var(--spacing-xl);
        font-size: var(--font-size-md);
        color: var(--color-text-secondary);
        line-height: var(--font-line-height-prose);
    }

    .info-box {
        background: var(--color-bg-tertiary);
        border-radius: var(--radius-lg);
        padding: var(--spacing-sm) var(--spacing-xl);
        margin-bottom: var(--spacing-xl);
    }

    .info-row {
        display: flex;
        justify-content: space-between;
        align-items: center;
        gap: var(--spacing-xl);
        padding: var(--spacing-md) 0;
    }

    .info-row:not(:last-child) {
        border-bottom: 1px solid var(--color-border-subtle);
    }

    .info-label {
        font-size: var(--font-size-md);
        color: var(--color-text-secondary);
        flex-shrink: 0;
    }

    .info-value {
        font-size: var(--font-size-md);
        color: var(--color-text-primary);
        font-weight: 500;
        text-align: right;
    }

    .validity-value {
        color: var(--color-toast-success-stripe);
    }

    .validity-value.validity-pending {
        color: var(--color-warning);
    }

    .validity-value.validity-expired {
        color: var(--color-error);
    }

    .info-row-sub {
        text-align: right;
        padding: 0 0 var(--spacing-md);
    }

    .info-hint {
        font-size: var(--font-size-sm);
        color: var(--color-text-tertiary);
        font-weight: 400;
    }

    .info-value.mono {
        font-family: var(--font-mono);
        letter-spacing: 0.02em;
    }

    .warning-banner {
        background: var(--color-warning-bg);
        border: 1px solid var(--color-warning);
        border-radius: var(--radius-lg);
        padding: var(--spacing-md) var(--spacing-lg);
        margin-bottom: var(--spacing-lg);
    }

    .warning-text {
        font-size: var(--font-size-md);
        color: var(--color-warning);
        line-height: var(--font-line-height-prose);
    }

    .input-group {
        margin-bottom: var(--spacing-lg);
    }

    .error-message {
        margin: 0 0 var(--spacing-sm);
        font-size: var(--font-size-md);
        color: var(--color-error);
        line-height: var(--font-line-height-prose);
        white-space: pre-wrap;
    }

    .help-text {
        margin: 0 0 var(--spacing-lg);
        font-size: var(--font-size-md);
        color: var(--color-text-secondary);
        line-height: var(--font-line-height-prose);
    }

    /* Whichever section ends the view, the footer's own inset is the gap below it. */
    .dialog-body > :global(:last-child) {
        margin-bottom: 0;
    }
</style>
