<!--
  Product § Feedback & errors (the page's `#inbox`): in-app feedback messages and error-report bundle
  metadata. Shows the counts, error reports per day, by kind, and per 100 installs by version; the feedback
  inbox and the recent-error-reports table sit behind closed-by-default `<details>`. Each feedback message with a reply-to email has a mark-answered toggle
  (the `setAnswered` action on `/product`). A message sent with logs names their `ERR-XXXXX`: a link to that
  row of the recent-error-reports table when it's listed there, else the id as selectable text.
-->
<script lang="ts">
    import type { SourceResult, DashboardSelection } from '#lib/server/types.js'
    import type { FeedbackAndErrorsData } from '#lib/server/sources/feedback-and-errors.js'
    import type { VersionWindow } from '#lib/server/sources/active-installs.js'
    import { enhance } from '$app/forms'
    import { errorReportsByDay, errorsPer100, inboxFeedback, tallyErrorReportsByField } from '#lib/feedback-and-errors.js'
    import { formatAverage, formatNumber } from '#lib/format.js'
    import { COLOR_GOLD } from '#lib/colors.js'
    import Chart from '#lib/components/Chart.svelte'
    import ErrorState from '#lib/components/ErrorState.svelte'
    import InfoPopover from '#lib/components/InfoPopover.svelte'
    import MetricRow from '#lib/components/MetricRow.svelte'
    import MetricTable from '#lib/components/MetricTable.svelte'
    import EmptyState from '#lib/components/EmptyState.svelte'
    import ExternalLinks from '#lib/components/ExternalLinks.svelte'

    const {
        feedbackAndErrors,
        versionWindow,
        selection,
        answerError = null,
    }: {
        feedbackAndErrors: SourceResult<FeedbackAndErrorsData>
        /**
         * The heartbeat's installs per version over the picker's complete days, for errors per 100; null when
         * the Worker doesn't send it yet (then the table counts reports only).
         */
        versionWindow: VersionWindow | null
        selection: DashboardSelection
        /** The mark-answered action's failure message, if the last toggle didn't save. */
        answerError?: string | null
    } = $props()

    /**
     * The recent-error-reports disclosure, opened by a feedback message's "Logs ERR-…" link so its row is
     * there to scroll to (not every browser opens a closed `<details>` for a fragment).
     */
    let reportsOpen = $state(false)

    const summaryClass = 'cursor-pointer text-sm font-medium text-text-secondary hover:text-text-primary'
</script>

<section id="inbox" class="@container scroll-mt-32 rounded-xl border border-border bg-surface p-4 sm:p-6">
    <h2 class="mb-4 flex items-center gap-2 text-lg font-semibold text-text-primary">
        Feedback &amp; errors
        <InfoPopover topic="feedback and errors">
            <p>What people sent through in-app feedback, and the error-report bundles rolling in.</p>
            <ul>
                <li>"Awaiting reply" counts every message with a reply-to address not yet marked answered, however old.</li>
                <li>Feedback carries no install id, so it can't be joined to analytics. Error reports use a separate diagnostics id.</li>
                <li>Both are low-volume, so a short window can look empty even when things are fine.</li>
            </ul>
            <p><span class="info-label">Source:</span> the api-server (feedback in D1, error bundles in R2, kept 90 days).</p>
        </InfoPopover>
    </h2>

    {#if feedbackAndErrors.ok}
        {@const fe = feedbackAndErrors.data}
        {@const errorsPerDay = errorReportsByDay(fe.errorReports)}
        {@const errorsByKind = tallyErrorReportsByField(fe.errorReports, 'kind')}
        {@const errorsByVersion = tallyErrorReportsByField(fe.errorReports, 'appVersion')}
        {@const rates = versionWindow ? errorsPer100(fe.allErrorReports, versionWindow).filter((r) => r.reports > 0) : null}
        {@const recentReports = fe.errorReports.slice(0, 15)}
        {@const inbox = inboxFeedback(fe.feedback, fe.awaitingReply, 15)}

        <MetricRow
            metrics={[
                { label: 'Feedback messages', value: formatNumber(fe.feedback.length), color: COLOR_GOLD },
                { label: 'Awaiting reply', value: formatNumber(fe.awaitingReply.length) },
                { label: 'Error reports', value: formatNumber(fe.errorReports.length) },
            ]}
        />

        {#if errorsPerDay.length > 1}
            <div class="mt-4">
                <h3 class="mb-2 text-sm font-medium text-text-secondary">Error reports per day</h3>
                <Chart
                    data={[errorsPerDay.map((d) => new Date(d.date).getTime() / 1000), errorsPerDay.map((d) => d.count)]}
                    labels={['Error reports']}
                    colors={[COLOR_GOLD]}
                    height={160}
                />
            </div>
        {/if}

        {#if fe.errorReports.length > 0}
            <div class="mt-4 grid gap-4 @2xl:grid-cols-2">
                <div>
                    <h3 class="mb-2 text-sm font-medium text-text-secondary">By kind</h3>
                    <MetricTable
                        items={errorsByKind.map((e) => ({ x: e.key, y: e.count }))}
                        colLabel="Kind"
                        colValue="Reports"
                    />
                </div>
                <div>
                    {#if rates && versionWindow}
                        <h3 class="mb-2 flex items-center gap-2 text-sm font-medium text-text-secondary">
                            By version, per 100 installs
                            <InfoPopover topic="errors per 100 installs" align="end">
                                <p>Error reports ÷ the installs that ran each version, over {versionWindow.since} – {versionWindow.until}.</p>
                                <ul>
                                    <li>Complete days only, so it can differ from the counts beside it, which include today.</li>
                                    <li>An install that upgraded counts under both versions.</li>
                                    <li>Below five installs on a version, the rate is noise: it shows a dash.</li>
                                </ul>
                                <p><span class="info-label">Source:</span> error bundles and the app's heartbeat.</p>
                            </InfoPopover>
                        </h3>
                        {#if rates.length > 0}
                            <table class="w-full text-left text-sm">
                                <thead>
                                    <tr class="border-b border-border-subtle text-xs text-text-tertiary">
                                        <th class="pr-4 pb-2 font-medium">Version</th>
                                        <th class="pr-4 pb-2 text-right font-medium">Reports</th>
                                        <th class="pr-4 pb-2 text-right font-medium">Installs</th>
                                        <th class="pb-2 text-right font-medium">Per 100</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {#each rates as rate (rate.version)}
                                        <tr class="border-b border-border-subtle/50">
                                            <td class="py-1.5 pr-4 text-text-primary tabular-nums">{rate.version}</td>
                                            <td class="py-1.5 pr-4 text-right text-text-secondary tabular-nums">{formatNumber(rate.reports)}</td>
                                            <td class="py-1.5 pr-4 text-right text-text-secondary tabular-nums">{formatNumber(rate.installs)}</td>
                                            <td class="py-1.5 text-right text-text-primary tabular-nums">
                                                {rate.per100 === null ? '–' : formatAverage(rate.per100)}
                                            </td>
                                        </tr>
                                    {/each}
                                </tbody>
                            </table>
                        {:else}
                            <p class="text-sm text-text-tertiary">No error reports on complete days in this range.</p>
                        {/if}
                    {:else}
                        <h3 class="mb-2 text-sm font-medium text-text-secondary">By version</h3>
                        <MetricTable
                            items={errorsByVersion.map((e) => ({ x: e.key, y: e.count }))}
                            colLabel="Version"
                            colValue="Reports"
                        />
                    {/if}
                </div>
            </div>
        {/if}

        {#if answerError}
            <p class="mt-4 text-sm text-danger">{answerError}</p>
        {/if}
        {#if inbox.length > 0}
            <details class="mt-4">
                <summary class={summaryClass}>
                    Recent feedback, and every message awaiting a reply
                    <span class="text-text-tertiary tabular-nums">({formatNumber(inbox.length)})</span>
                </summary>
                <div class="mt-2 space-y-2">
                    {#each inbox as msg (msg.id)}
                        <div class="rounded-lg border border-border-subtle bg-surface-elevated px-3 py-2">
                            <p class="text-sm whitespace-pre-wrap text-text-primary">{msg.feedback}</p>
                            <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-tertiary">
                                <span class="tabular-nums">{msg.createdAt.split(' ')[0]}</span>
                                <span>v{msg.appVersion}</span>
                                <span>{msg.osVersion}</span>
                                {#if msg.errorReportId}
                                    {#if recentReports.some((report) => report.id === msg.errorReportId)}
                                        <a
                                            class="text-accent hover:text-accent-hover"
                                            href="#{msg.errorReportId}"
                                            onclick={() => (reportsOpen = true)}
                                        >
                                            Logs <span class="font-mono">{msg.errorReportId}</span>
                                        </a>
                                    {:else}
                                        <span>Logs <span class="font-mono select-all">{msg.errorReportId}</span></span>
                                    {/if}
                                {/if}
                                {#if msg.email}
                                    <a
                                        class="text-accent hover:text-accent-hover"
                                        href="mailto:{msg.email}?subject=Re%3A%20your%20Cmdr%20feedback"
                                    >
                                        Reply to {msg.email}
                                    </a>
                                    {#if msg.answeredAt}
                                        <span class="text-text-secondary">
                                            Answered <span class="tabular-nums">{msg.answeredAt.split(' ')[0]}</span>
                                        </span>
                                    {/if}
                                    <form
                                        method="POST"
                                        action="?/setAnswered"
                                        use:enhance={() =>
                                            ({ update }) =>
                                                update({ reset: false })}
                                    >
                                        <input type="hidden" name="id" value={msg.id} />
                                        <input type="hidden" name="answered" value={msg.answeredAt ? 'false' : 'true'} />
                                        <button
                                            type="submit"
                                            class="rounded-md border border-border px-2 py-0.5 text-xs text-text-secondary
                                                transition-colors hover:text-text-primary"
                                        >
                                            {msg.answeredAt ? 'Mark as awaiting reply' : 'Mark answered'}
                                        </button>
                                    </form>
                                {/if}
                            </div>
                        </div>
                    {/each}
                </div>
            </details>
        {:else}
            <EmptyState />
        {/if}

        {#if fe.errorReports.length > 0}
            <details class="mt-4" bind:open={reportsOpen}>
                <summary class={summaryClass}>
                    Recent error reports
                    <span class="text-text-tertiary tabular-nums">({formatNumber(recentReports.length)})</span>
                </summary>
                <div class="mt-2 overflow-x-auto">
                    <table class="w-full text-left text-sm">
                        <thead>
                            <tr class="border-b border-border-subtle text-text-tertiary">
                                <th class="pr-4 pb-2 font-medium">ID</th>
                                <th class="pr-4 pb-2 font-medium">Kind</th>
                                <th class="pr-4 pb-2 font-medium">Version</th>
                                <th class="pb-2 text-right font-medium">Date</th>
                            </tr>
                        </thead>
                        <tbody>
                            {#each recentReports as report (report.id)}
                                <tr id={report.id} class="scroll-mt-32 border-b border-border-subtle/50 target:bg-accent/10">
                                    <td class="py-1.5 pr-4 font-mono text-text-primary">{report.id}</td>
                                    <td class="py-1.5 pr-4 text-text-secondary">{report.kind}</td>
                                    <td class="py-1.5 pr-4 text-text-secondary tabular-nums">{report.appVersion}</td>
                                    <td class="py-1.5 text-right text-text-tertiary tabular-nums">{report.date}</td>
                                </tr>
                            {/each}
                        </tbody>
                    </table>
                </div>
            </details>
        {/if}

        <ExternalLinks links={[{ label: 'View bundles in Cloudflare R2', href: 'https://dash.cloudflare.com' }]} />
    {:else}
        <ErrorState error={feedbackAndErrors.error} {selection} />
    {/if}
</section>
