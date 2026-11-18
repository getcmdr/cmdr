<script lang="ts">
    import { onMount } from 'svelte'
    import SettingsSection from '../components/SettingsSection.svelte'
    import {
        openExternalUrl,
        getLicenseInfo,
        getLicenseStatus,
        emitExecuteCommand,
        type LicenseInfo,
        type LicenseStatus,
    } from '#lib/tauri-commands/index.js'
    import Button from '#lib/ui/Button.svelte'
    import SectionCard from '#lib/ui/SectionCard.svelte'
    import Icon from '#lib/ui/Icon.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { getLicenseTypeLabel, getStatusText } from './license-section-utils'
    import { closeSelfWindow } from '#lib/child-window-close.js'

    let licenseInfo = $state<LicenseInfo | null>(null)
    let licenseStatus = $state<LicenseStatus | null>(null)
    let isLoading = $state(true)

    onMount(async () => {
        try {
            const [info, status] = await Promise.all([
                getLicenseInfo().catch(() => null),
                getLicenseStatus().catch(() => null),
            ])
            licenseInfo = info
            licenseStatus = status
        } finally {
            isLoading = false
        }
    })

    const hasLicense = $derived(licenseInfo !== null)

    // What the free Personal license covers, so the two buttons below read as the next step for
    // someone who needs more. Facts from `docs/business/pricing.md` § The live tiers and
    // `licensing.md` § Additional Use Grants: keep these in step with them, and keep the AI line's
    // "free during the beta" qualifier (pricing.md says why).
    const personalIncludes = $derived([
        tString('licensing.section.includedFeatures'),
        tString('licensing.section.includedMacs'),
        tString('licensing.section.includedUpdates'),
        tString('licensing.section.includedAi'),
    ])
    const statusText = $derived(getStatusText(licenseStatus))

    async function handleManageLicense() {
        // Cross-window `execute-command` emit: this `commandId` must stay a valid frontend
        // `COMMAND_IDS` entry (it's narrowed by `isCommandId` in `+page.svelte` before
        // dispatch). The `rust-command-id-drift.test.ts` test pins it to the registry.
        await emitExecuteCommand('app.licenseKey')
        // Like every other settings-window self-close: the backend hides the window and destroys
        // it a moment later. See `#lib/child-window-close`.
        void closeSelfWindow()
    }

    async function handleBuyLicense() {
        await openExternalUrl('https://getcmdr.com/pricing')
    }
</script>

<SettingsSection title={tString('licensing.section.title')}>
    {#if isLoading}
        <p class="loading-text">{tString('licensing.section.loading')}</p>
    {:else}
        <SectionCard>
            <div class="license-info">
                <div class="info-row">
                    <span class="info-label">{tString('licensing.section.labelType')}</span>
                    <span class="info-value">{getLicenseTypeLabel(licenseInfo)}</span>
                </div>
                {#if licenseInfo?.organizationName}
                    <div class="info-row">
                        <span class="info-label">{tString('licensing.section.labelOrganization')}</span>
                        <span class="info-value">{licenseInfo.organizationName}</span>
                    </div>
                {/if}
                {#if statusText}
                    <div class="info-row">
                        <span class="info-label">{tString('licensing.section.labelStatus')}</span>
                        <span
                            class="info-value"
                            class:status-expired={licenseStatus?.type === 'expired'}
                            class:status-active={licenseStatus?.type === 'commercial'}>{statusText}</span
                        >
                    </div>
                {/if}
                {#if licenseInfo?.shortCode}
                    <div class="info-row">
                        <span class="info-label">{tString('licensing.section.labelKey')}</span>
                        <span class="info-value mono">{licenseInfo.shortCode}</span>
                    </div>
                {/if}
            </div>

            {#if !hasLicense}
                <div class="included">
                    <h3 class="included-title">{tString('licensing.section.includedTitle')}</h3>
                    <ul class="included-list">
                        {#each personalIncludes as item (item)}
                            <li>
                                <span class="included-glyph"><Icon name="check" size={14} aria-hidden="true" /></span>
                                <span>{item}</span>
                            </li>
                        {/each}
                    </ul>
                    <p class="commercial-line">{tString('licensing.section.commercialNeeded')}</p>
                </div>
            {/if}

            <div class="actions">
                {#if hasLicense}
                    <Button variant="secondary" size="mini" onclick={handleManageLicense}
                        >{tString('licensing.section.manageKey')}</Button
                    >
                {:else}
                    <Button variant="secondary" size="mini" onclick={handleManageLicense}
                        >{tString('licensing.section.enterKey')}</Button
                    >
                    <Button variant="secondary" size="mini" onclick={handleBuyLicense}
                        >{tString('licensing.section.getLicense')}</Button
                    >
                {/if}
            </div>
        </SectionCard>
    {/if}
</SettingsSection>

<style>
    .loading-text {
        color: var(--color-text-tertiary);
        font-size: var(--font-size-sm);
        margin: 0;
    }

    /* No background/border/radius of its own: the wrapping SectionCard owns the
       card frame, so this is just the rows-plus-separator block inside it. */
    .license-info {
        margin-bottom: var(--spacing-md);
    }

    .info-row {
        display: flex;
        justify-content: space-between;
        align-items: center;
        gap: var(--spacing-xl);
        padding: var(--spacing-sm) 0;
    }

    .info-row:not(:last-child) {
        border-bottom: 1px solid var(--color-border-subtle);
    }

    .info-label {
        font-size: var(--font-size-sm);
        color: var(--color-text-secondary);
        flex-shrink: 0;
    }

    .info-value {
        font-size: var(--font-size-sm);
        color: var(--color-text-primary);
        font-weight: 500;
        text-align: right;
    }

    .info-value.mono {
        font-family: var(--font-mono);
        letter-spacing: 0.02em;
    }

    .info-value.status-expired {
        color: var(--color-error);
    }

    .info-value.status-active {
        color: var(--color-toast-success-stripe);
    }

    .included {
        margin-bottom: var(--spacing-md);
    }

    .included-title {
        margin: 0 0 var(--spacing-xs);
        font-size: var(--font-size-sm);
        font-weight: 600;
        color: var(--color-text-primary);
    }

    .included-list {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xxs);
        margin: 0 0 var(--spacing-sm);
        padding: 0;
        list-style: none;
        font-size: var(--font-size-sm);
        color: var(--color-text-primary);
    }

    .included-list li {
        display: flex;
        align-items: center;
        gap: var(--spacing-xs);
    }

    .included-glyph {
        display: inline-flex;
        flex: none;
        color: var(--color-allow);
    }

    .commercial-line {
        margin: 0;
        font-size: var(--font-size-sm);
        color: var(--color-text-secondary);
    }

    .actions {
        display: flex;
        flex-wrap: wrap;
        gap: var(--spacing-sm);
    }
</style>
