<script lang="ts">
    import SettingsSection from '../components/SettingsSection.svelte'
    import SettingRow from '../components/SettingRow.svelte'
    import SettingSwitch from '../components/SettingSwitch.svelte'
    import { getSettingDefinition } from '#lib/settings/index.js'
    import { createShouldShow, anyVisible } from '#lib/settings/settings-search.js'
    import SectionCard from '#lib/ui/SectionCard.svelte'
    import Button from '#lib/ui/Button.svelte'
    import TextInput from '#lib/ui/TextInput.svelte'
    import { updateState, checkForUpdates } from '#lib/updates/updater.svelte.js'
    import {
        describeUpdateFailure,
        formatUpdateStatus,
        updateFailureOffersReport,
    } from '#lib/updates/update-status-text.js'
    import ReportOffer from '#lib/error-reporter/ReportOffer.svelte'
    import { createBetaEmailSignup } from './beta-email-signup.svelte'
    import { tString } from '#lib/intl/messages.svelte.js'
    import { getManagedPolicyView } from '#lib/managed-policy/managed-policy.svelte.js'
    import ManagedPolicySummary from '#lib/managed-policy/ManagedPolicySummary.svelte'
    import DeleteAllDataDialog from './DeleteAllDataDialog.svelte'
    import { createDataWipeFlow } from './delete-all-data-flow.svelte'
    import { revealSecurityLog } from '#lib/tauri-commands/index.js'
    import { getAppLogger } from '#lib/logging/logger.js'

    interface Props {
        searchQuery: string
    }

    const { searchQuery }: Props = $props()

    const shouldShow = $derived(createShouldShow(searchQuery))

    const autoCheckDef = getSettingDefinition('updates.autoCheck') ?? { label: '', description: '' }
    const whatsNewDef = getSettingDefinition('whatsNew.showOnUpdate') ?? { label: '', description: '' }
    const analyticsDef = getSettingDefinition('analytics.enabled') ?? { label: '', description: '' }
    const emailDef = getSettingDefinition('analytics.email') ?? { label: '', description: '' }
    const crashReportsDef = getSettingDefinition('updates.crashReports') ?? { label: '', description: '' }
    const errorReportsDef = getSettingDefinition('updates.errorReports') ?? { label: '', description: '' }
    const securityLogDef = getSettingDefinition('securityLog.enabled') ?? { label: '', description: '' }

    // Under the organization's `DisableUpdates` a check could only say so, so the button stays off and
    // the status line says why up front. A build already staged keeps its own line: it still applies.
    const updatesOff = $derived(getManagedPolicyView().updates.kind === 'disabled')
    const statusText = $derived(
        updatesOff && updateState.status === 'idle' && updateState.failure === null
            ? tString('updates.status.managedOff')
            : formatUpdateStatus(updateState),
    )
    const failureText = $derived(updateState.failure === null ? null : describeUpdateFailure(updateState.failure))
    const offersReport = $derived(updateState.failure !== null && updateFailureOffersReport(updateState.failure))
    const buttonDisabled = $derived(updateState.status !== 'idle' || updatesOff)
    const statusId = 'updates-check-status'

    // The beta contact email field: persists on every keystroke, subscribes on commit. The logic is
    // shared with the onboarding sheet's `StepBeta`, so both surfaces behave identically.
    const emailSignup = createBetaEmailSignup()

    function handleCheckForUpdates() {
        void checkForUpdates('settings')
    }

    async function handleShowSecurityLog() {
        const refusal = await revealSecurityLog()
        if (refusal !== null) {
            getAppLogger('settings').warn('Couldn’t show the security event log: {kind}', { kind: refusal.kind })
        }
    }

    const dataWipe = createDataWipeFlow()
    const dataWipeDescriptionId = 'data-wipe-description'
</script>

<SettingsSection title={tString('settings.section.updatesAndPrivacy')}>
    <!-- Not searchable: it renders only on a managed Mac, and a static search entry would hit on
         every other one. A search hides it with the cards it doesn't match. -->
    {#if searchQuery.trim() === ''}
        <ManagedPolicySummary />
    {/if}
    {#if anyVisible(shouldShow, 'row:updates.checkForUpdates', 'updates.autoCheck', 'whatsNew.showOnUpdate')}
        <SectionCard label={tString('settings.updates.card.updates')}>
            <!-- Not a setting, so it carries a searchable-row id (`UpdatesSection.rows.ts`)
                 and rides the same `shouldShow` gate as the switches below it. -->
            {#if shouldShow('row:updates.checkForUpdates')}
                <div class="check-row">
                    <Button
                        variant="secondary"
                        size="mini"
                        onclick={handleCheckForUpdates}
                        disabled={buttonDisabled}
                        aria-describedby={statusId}
                    >
                        {tString('settings.updates.checkForUpdates')}
                    </Button>
                    <div class="status" id={statusId}>
                        {#if failureText !== null}
                            <span class="failure-message">{failureText}</span>
                            {#if offersReport}
                                <!-- The note is the sentence the row showed; the raw detail is already in the
                                     log the report bundles. This window doesn't host the dialog, so the
                                     main window opens it (`error-report-flow.svelte.ts`). -->
                                <ReportOffer
                                    incident={{ kind: 'message', from: 'updateFailure', text: failureText }}
                                    look="link"
                                />
                            {/if}
                        {:else if statusText}
                            <span class="status-text">{statusText}</span>
                        {/if}
                    </div>
                </div>
            {/if}
            {#if shouldShow('updates.autoCheck')}
                <SettingRow
                    id="updates.autoCheck"
                    label={autoCheckDef.label}
                    description={autoCheckDef.description}
                    {searchQuery}
                >
                    <SettingSwitch id="updates.autoCheck" />
                </SettingRow>
            {/if}
            {#if shouldShow('whatsNew.showOnUpdate')}
                <SettingRow
                    id="whatsNew.showOnUpdate"
                    label={whatsNewDef.label}
                    description={whatsNewDef.description}
                    {searchQuery}
                >
                    <SettingSwitch id="whatsNew.showOnUpdate" />
                </SettingRow>
            {/if}
        </SectionCard>
    {/if}

    {#if anyVisible(shouldShow, 'analytics.enabled', 'analytics.email', 'updates.crashReports', 'updates.errorReports')}
        <SectionCard label={tString('settings.updates.card.privacyAndDataSharing')}>
            {#if shouldShow('analytics.enabled')}
                <SettingRow
                    id="analytics.enabled"
                    label={analyticsDef.label}
                    description={analyticsDef.description}
                    {searchQuery}
                >
                    <SettingSwitch id="analytics.enabled" />
                </SettingRow>
            {/if}
            {#if shouldShow('analytics.email')}
                <SettingRow
                    id="analytics.email"
                    label={emailDef.label}
                    description={emailDef.description}
                    {searchQuery}
                >
                    <TextInput
                        type="email"
                        placeholder={tString('settings.updates.emailPlaceholder')}
                        value={emailSignup.email}
                        oninput={emailSignup.handleInput}
                        onblur={emailSignup.handleBlur}
                        onkeydown={emailSignup.handleKeydown}
                        disabled={emailSignup.signupInFlight}
                        ariaLabel={emailDef.label}
                        autocomplete="email"
                        autocapitalize="off"
                        spellcheck={false}
                    />
                </SettingRow>
                {#if emailSignup.signupFeedback?.kind === 'success'}
                    <p class="signup-feedback success" role="status">
                        {tString('settings.updates.emailConfirmHint')}
                    </p>
                {:else if emailSignup.signupFeedback?.kind === 'failure'}
                    <p class="signup-feedback failure" role="status">
                        {tString('settings.updates.emailSignupError')}
                    </p>
                {/if}
                <p class="email-note">
                    {tString('settings.updates.emailPrivacyNote')}
                </p>
            {/if}
            {#if shouldShow('updates.crashReports')}
                <SettingRow
                    id="updates.crashReports"
                    label={crashReportsDef.label}
                    description={crashReportsDef.description}
                    {searchQuery}
                >
                    <SettingSwitch id="updates.crashReports" />
                </SettingRow>
            {/if}
            {#if shouldShow('updates.errorReports')}
                <SettingRow
                    id="updates.errorReports"
                    label={errorReportsDef.label}
                    description={errorReportsDef.description}
                    {searchQuery}
                >
                    <SettingSwitch id="updates.errorReports" />
                </SettingRow>
            {/if}
        </SectionCard>
    {/if}

    {#if anyVisible(shouldShow, 'securityLog.enabled', 'row:updates.showSecurityLog')}
        <SectionCard label={tString('settings.updates.card.securityEvents')}>
            {#if shouldShow('securityLog.enabled')}
                <SettingRow
                    id="securityLog.enabled"
                    label={securityLogDef.label}
                    description={securityLogDef.description}
                    {searchQuery}
                >
                    <SettingSwitch id="securityLog.enabled" />
                </SettingRow>
            {/if}
            <!-- Not a setting: a searchable row (`UpdatesSection.rows.ts`). -->
            {#if shouldShow('row:updates.showSecurityLog')}
                <div class="security-log-row">
                    <Button variant="secondary" size="mini" onclick={() => void handleShowSecurityLog()}>
                        {tString('settings.securityLog.showInFinder')}
                    </Button>
                </div>
            {/if}
        </SectionCard>
    {/if}

    <!-- "Delete all Cmdr data": an action, not a setting, so a searchable row (`UpdatesSection.rows.ts`). -->
    {#if shouldShow('row:updates.deleteAllData')}
        <SectionCard label={tString('settings.dataWipe.card')}>
            <div class="data-wipe-row">
                <p class="data-wipe-description" id={dataWipeDescriptionId}>
                    {tString('settings.dataWipe.description')}
                </p>
                <Button
                    variant="secondary"
                    size="mini"
                    disabled={dataWipe.opening}
                    aria-describedby={dataWipeDescriptionId}
                    onclick={() => void dataWipe.open()}
                >
                    {tString('settings.dataWipe.button')}
                </Button>
            </div>
        </SectionCard>
    {/if}
    {#if dataWipe.view}
        <DeleteAllDataDialog
            view={dataWipe.view}
            onConfirm={() => void dataWipe.confirm()}
            onCancel={dataWipe.cancel}
            onQuit={() => void dataWipe.quit()}
        />
    {/if}
</SettingsSection>

<style>
    .check-row {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        margin-bottom: var(--spacing-md);
    }

    .status {
        display: flex;
        flex-direction: column;
        gap: var(--spacing-xs);
        font-size: var(--font-size-sm);
        color: var(--color-text-secondary);
        min-height: 1.4em;
    }

    .failure-message {
        color: var(--color-text-primary);
    }

    .signup-feedback {
        margin: var(--spacing-xs) 0 0;
        font-size: var(--font-size-sm);
    }

    .signup-feedback.success {
        color: var(--color-toast-success-stripe);
    }

    .signup-feedback.failure {
        color: var(--color-text-primary);
    }

    .security-log-row {
        margin-top: var(--spacing-sm);
    }

    .data-wipe-row {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: var(--spacing-sm);
    }

    .data-wipe-description {
        margin: 0;
        font-size: var(--font-size-sm);
        color: var(--color-text-secondary);
    }

    .email-note {
        margin: var(--spacing-xs) 0 var(--spacing-md);
        font-size: var(--font-size-xs);
        color: var(--color-text-secondary);
    }
</style>
