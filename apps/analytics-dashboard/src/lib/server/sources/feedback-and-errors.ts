import type { DashboardSelection, SourceResult } from '../types.js'
import { rangeDays } from '../types.js'
import { isAwaitingReply, type FeedbackRow, type ErrorReportRow } from '../../feedback-and-errors.js'
import { cacheGet, cacheSet } from '../cache.js'
import { fetchWorkerEndpoint } from './worker-endpoint.js'

export interface FeedbackAndErrorsData {
  /** Feedback from the range (the last N × 24 hours), newest first. */
  feedback: FeedbackRow[]
  /** Error reports dated within the range's days, newest first. */
  errorReports: ErrorReportRow[]
  /** Every message still owed a reply, however old: the picker must never hide one. */
  awaitingReply: FeedbackRow[]
  /** Every error report R2 still holds (it keeps 90 days), for counts that start at a release. */
  allErrorReports: ErrorReportRow[]
}

interface FeedbackAndErrorsEnv {
  LICENSE_SERVER_ADMIN_TOKEN: string
  /** Optional override for the api-server base URL (local QA). Defaults to production. */
  WORKER_BASE_URL?: string
}

/**
 * Feedback and error reports, each fetched over `range=all` once and windowed to the range here, so the
 * "Awaiting reply" count and the list come from one response, and a count from a release's publish time
 * doesn't depend on the picker. The windows match the Worker's own: feedback from the last N × 24 hours
 * (`created_at >= datetime('now', '-N days')`), error reports from the UTC day N days back on.
 *
 * Feedback is read live on every load and only the error-report listing is cached. A message
 * marked answered has to show as answered on the reload right after, and a cached feedback list
 * would keep it under "Awaiting reply" for up to an hour. The feedback query is one small D1 read;
 * the error-report listing is an R2 scan, so that's the half worth caching.
 */
export async function fetchFeedbackAndErrorsData(
  env: FeedbackAndErrorsEnv,
  selection: DashboardSelection,
  now: Date = new Date(),
): Promise<SourceResult<FeedbackAndErrorsData>> {
  try {
    const [allFeedback, allErrorReports] = await Promise.all([
      fetchWorkerEndpoint<FeedbackRow[]>(
        env.LICENSE_SERVER_ADMIN_TOKEN,
        '/admin/feedback?range=all',
        env.WORKER_BASE_URL,
      ),
      fetchAllErrorReports(env),
    ])
    const cutoff = new Date(now.getTime() - rangeDays[selection.range] * 86_400_000)
    // D1's `datetime()` format (`YYYY-MM-DD HH:MM:SS`, UTC) compares as a string.
    const feedbackCutoff = cutoff.toISOString().replace('T', ' ').slice(0, 19)
    const errorCutoff = cutoff.toISOString().slice(0, 10)
    return {
      ok: true,
      data: {
        feedback: allFeedback.filter((row) => row.createdAt >= feedbackCutoff),
        errorReports: allErrorReports.filter((row) => row.date >= errorCutoff),
        awaitingReply: allFeedback.filter(isAwaitingReply),
        allErrorReports,
      },
    }
  } catch (e) {
    return { ok: false, error: `Feedback & errors: ${e instanceof Error ? e.message : String(e)}` }
  }
}

async function fetchAllErrorReports(env: FeedbackAndErrorsEnv): Promise<ErrorReportRow[]> {
  const cached = await cacheGet<ErrorReportRow[]>('error-reports', 'all')
  if (cached) return cached

  const errorReports = await fetchWorkerEndpoint<ErrorReportRow[]>(
    env.LICENSE_SERVER_ADMIN_TOKEN,
    '/admin/error-reports?range=all',
    env.WORKER_BASE_URL,
  )
  await cacheSet('error-reports', 'all', errorReports)
  return errorReports
}

/**
 * Marks one feedback message answered, or back to awaiting a reply (`PUT /admin/feedback/:id/answered`).
 * Returns the stored `answeredAt` (null when unmarked). The worker's message comes back verbatim on a
 * rejection so the page shows what the server actually objected to.
 */
export async function setFeedbackAnswered(
  env: FeedbackAndErrorsEnv,
  input: { id: number; answered: boolean },
): Promise<SourceResult<string | null>> {
  const baseUrl = env.WORKER_BASE_URL || 'https://api.getcmdr.com'
  try {
    const response = await fetch(`${baseUrl}/admin/feedback/${String(input.id)}/answered`, {
      method: 'PUT',
      headers: {
        Authorization: `Bearer ${env.LICENSE_SERVER_ADMIN_TOKEN}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ answered: input.answered }),
    })
    const text = await response.text()
    if (!response.ok) throw new Error(errorMessage(text) ?? `returned ${String(response.status)}`)
    return { ok: true, data: (JSON.parse(text) as { answeredAt: string | null }).answeredAt }
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) }
  }
}

/** The worker's `{ error }` string, or null when the body isn't the shape we expect. */
function errorMessage(body: string): string | null {
  try {
    const parsed = JSON.parse(body) as { error?: unknown }
    return typeof parsed.error === 'string' ? parsed.error : null
  } catch {
    return null
  }
}
