/**
 * Domain types and pure aggregation helpers for in-app feedback and error reports. Lives outside
 * `#lib/server` so both the page (client bundle) and the report/server code can import the helpers;
 * the server-only fetching lives in `#lib/server/sources/feedback-and-errors.ts`.
 */
import type { VersionWindow } from './server/sources/active-installs.js'
import { compareVersions } from './rollout.js'

/**
 * One in-app "Send feedback" submission. `email` is the optional reply-to the sender attached.
 * `answeredAt` is when its board card stopped owing a reply (closed, or `needs-reply` taken off), which
 * the api-server's cron copies from the card; null while it still needs one, or when it has no card. `errorReportId` is the `ERR-XXXXX` of the
 * logs the sender chose to include, or null when they sent none.
 */
export interface FeedbackRow {
  id: number
  createdAt: string
  feedback: string
  email: string | null
  appVersion: string
  osVersion: string
  buildMode: string | null
  answeredAt: string | null
  errorReportId: string | null
}

/** One error-report bundle's metadata (the zip itself stays in R2). `kind` is "auto" or "user". */
export interface ErrorReportRow {
  id: string
  kind: string
  appVersion: string
  osVersion: string
  arch: string
  date: string
  generatedAt: string
}

/** Whether a message still owes its sender a reply: it carries a reply-to email and has no `answeredAt` (its card still owes one). */
export function isAwaitingReply(row: FeedbackRow): boolean {
  return row.email != null && row.email !== '' && row.answeredAt == null
}

/**
 * The inbox list: the range's newest `limit` messages, plus every message still awaiting a reply that isn't
 * among them (however old), newest first. So a message the top strip counts is always in the list it links to.
 */
export function inboxFeedback(inRange: FeedbackRow[], awaitingReply: FeedbackRow[], limit: number): FeedbackRow[] {
  const newest = [...inRange].sort((a, b) => b.createdAt.localeCompare(a.createdAt)).slice(0, limit)
  const listed = new Set(newest.map((row) => row.id))
  return [...newest, ...awaitingReply.filter((row) => !listed.has(row.id))].sort((a, b) =>
    b.createdAt.localeCompare(a.createdAt),
  )
}

/** Tallies error reports by a field (kind/version/arch), highest count first. */
export function tallyErrorReportsByField(
  rows: ErrorReportRow[],
  field: 'kind' | 'appVersion' | 'arch',
): Array<{ key: string; count: number }> {
  const counts = new Map<string, number>()
  for (const row of rows) {
    const key = row[field] || '(unknown)'
    counts.set(key, (counts.get(key) ?? 0) + 1)
  }
  return [...counts.entries()].map(([key, count]) => ({ key, count })).sort((a, b) => b.count - a.count)
}

/** Error reports grouped by day, oldest first (for the timeline chart). */
export function errorReportsByDay(rows: ErrorReportRow[]): Array<{ date: string; count: number }> {
  const counts = new Map<string, number>()
  for (const row of rows) {
    counts.set(row.date, (counts.get(row.date) ?? 0) + 1)
  }
  return [...counts.entries()].map(([date, count]) => ({ date, count })).sort((a, b) => a.date.localeCompare(b.date))
}

/** Below this many installs on a version, a rate is noise: print the raw count. */
export const minInstallsForRate = 5

/** One version's error reports against the installs that ran it. */
export interface VersionErrorRate {
  version: string
  reports: number
  /** Distinct installs with any beat on this version in the window. */
  installs: number
  /** Reports per 100 installs, or null below `minInstallsForRate` installs. */
  per100: number | null
}

/**
 * Error reports per 100 installs, per version, over `window`'s complete days (its `since`–`until`, both
 * inclusive): reports dated in the window ÷ the installs that ran that version in it. Newest version first.
 */
export function errorsPer100(rows: ErrorReportRow[], window: VersionWindow): VersionErrorRate[] {
  const reports = new Map<string, number>()
  for (const row of rows) {
    if (row.date < window.since || row.date > window.until) continue
    reports.set(row.appVersion, (reports.get(row.appVersion) ?? 0) + 1)
  }
  return [...new Set([...reports.keys(), ...Object.keys(window.byVersion)])]
    .map((version) => {
      const count = reports.get(version) ?? 0
      const installs = window.byVersion[version] ?? 0
      return {
        version,
        reports: count,
        installs,
        per100: installs >= minInstallsForRate ? (count * 100) / installs : null,
      }
    })
    .sort((a, b) => compareVersions(b.version, a.version))
}

/** Error reports generated at or after a release's publish time, whatever the picker's range. */
export function errorsSinceRelease(rows: ErrorReportRow[], publishedAt: string): number {
  const since = Date.parse(publishedAt)
  return rows.filter((row) => Date.parse(row.generatedAt) >= since).length
}
