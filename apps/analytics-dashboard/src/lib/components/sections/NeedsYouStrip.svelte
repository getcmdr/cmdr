<!--
  Product § the line on top: what needs David now. The messages still owed a reply (however old) and the
  error reports since the latest release, each linking to the inbox below. A part at zero, or on a source
  that failed, is left out, and the strip renders nothing when both are.
-->
<script lang="ts">
    import type { SourceResult } from '#lib/server/types.js'
    import type { FeedbackAndErrorsData } from '#lib/server/sources/feedback-and-errors.js'
    import type { LatestRelease } from '#lib/rollout.js'
    import { errorsSinceRelease } from '#lib/feedback-and-errors.js'
    import { formatNumber } from '#lib/format.js'

    interface Props {
        feedbackAndErrors: SourceResult<FeedbackAndErrorsData>
        /** The newest stable release; without a publish date there's no "since" to count from. */
        release: LatestRelease | null
    }

    const { feedbackAndErrors, release }: Props = $props()

    const parts = $derived.by(() => {
        if (!feedbackAndErrors.ok) return []
        const awaiting = feedbackAndErrors.data.awaitingReply.length
        const errors =
            release?.publishedAt != null ? errorsSinceRelease(feedbackAndErrors.data.allErrorReports, release.publishedAt) : 0
        return [
            ...(awaiting > 0 ? [`${formatNumber(awaiting)} ${awaiting === 1 ? 'message' : 'messages'} awaiting a reply`] : []),
            ...(errors > 0 && release
                ? [`${formatNumber(errors)} new error ${errors === 1 ? 'report' : 'reports'} since v${release.version}`]
                : []),
        ]
    })
</script>

{#if parts.length > 0}
    <p class="rounded-xl border border-accent/40 bg-accent/10 px-4 py-2.5 text-sm text-text-primary">
        {#each parts as part, i (part)}
            {#if i > 0}<span class="mx-2 text-text-tertiary">·</span>{/if}
            <a class="font-medium hover:text-accent-hover hover:underline" href="#inbox">{part}</a>
        {/each}
    </p>
{/if}
