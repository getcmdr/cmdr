/**
 * Client-safe release rollout: how far the latest release has spread through the heartbeat population, and
 * the per-version stack behind the rollout chart. Both read the heartbeat's per-day `byVersion` (each
 * install under the highest version it ran that day), so the headline and the chart are one population.
 * Like every product stat, the headline reads yesterday, never today's partial day.
 */
import type { ActiveInstallsDay } from './server/sources/active-installs.js'
import type { GitHubRelease } from './server/sources/github.js'
import { shiftDay } from './time-range.js'
import { formatPercent } from './format.js'

/** The release the headline tracks. `publishedAt` is null for the fallback (a version seen, no release). */
export interface LatestRelease {
  /** Without the tag's `v`, as heartbeats report `app_version`. */
  version: string
  publishedAt: string | null
}

export type RolloutHeadline =
  /** Published today: no complete day exists on it yet, so no share ("first full day tomorrow"). */
  | { kind: 'releasedToday'; version: string }
  /** Published yesterday: the share is of a day that was partly before the release. */
  | { kind: 'onReleaseDay'; version: string; share: number | null }
  | { kind: 'daysAfter'; version: string; daysAfter: number; share: number | null }
  /** The fallback version, which has no publish date to count from. */
  | { kind: 'noReleaseDate'; version: string; share: number | null }

/**
 * A total order on version strings, by numeric dot-separated parts, matching the Worker's
 * `compareVersions` (`apps/api-server/src/admin/active-installs.ts`). A part that isn't all digits (like
 * `0-beta` in `0.62.0-beta.1`) sorts below any number, so a pre-release sits below its release.
 */
export function compareVersions(a: string, b: string): number {
  const partsA = a.split('.')
  const partsB = b.split('.')
  for (let i = 0; i < Math.max(partsA.length, partsB.length); i++) {
    const order = comparePart(partsA.at(i), partsB.at(i))
    if (order !== 0) return order
  }
  return a < b ? -1 : a > b ? 1 : 0
}

function comparePart(a: string | undefined, b: string | undefined): number {
  const numA = a !== undefined && /^\d+$/.test(a) ? Number(a) : null
  const numB = b !== undefined && /^\d+$/.test(b) ? Number(b) : null
  if (numA !== null && numB !== null) return numA - numB
  if (numA !== null) return 1
  if (numB !== null) return -1
  const textA = a ?? ''
  const textB = b ?? ''
  return textA < textB ? -1 : textA > textB ? 1 : 0
}

/**
 * The newest stable GitHub release (drafts and prereleases skipped), or, without one, the newest version
 * any install ran. `releases` is null when GitHub couldn't be read.
 */
export function latestRelease(releases: GitHubRelease[] | null, days: ActiveInstallsDay[]): LatestRelease | null {
  const stable = (releases ?? [])
    .filter((release) => !release.draft && !release.prerelease)
    .sort((a, b) => b.publishedAt.localeCompare(a.publishedAt))
  if (stable.length > 0) return { version: stable[0].tagName.replace(/^v/, ''), publishedAt: stable[0].publishedAt }

  const seen = days.flatMap((d) => Object.keys(d.byVersion)).sort(compareVersions)
  return seen.length > 0 ? { version: seen[seen.length - 1], publishedAt: null } : null
}

/** The headline: `release`'s share of yesterday's active installs, and how long after its release. */
export function rolloutHeadline(release: LatestRelease, days: ActiveInstallsDay[], today: string): RolloutHeadline {
  const yesterday = shiftDay(today, -1)
  const row = days.find((d) => d.date === yesterday)
  const share = row && row.dau > 0 ? (row.byVersion[release.version] ?? 0) / row.dau : null
  if (release.publishedAt === null) return { kind: 'noReleaseDate', version: release.version, share }

  const publishDay = new Date(release.publishedAt).toISOString().slice(0, 10)
  const daysAfter = Math.round(
    (Date.parse(`${yesterday}T00:00:00Z`) - Date.parse(`${publishDay}T00:00:00Z`)) / 86_400_000,
  )
  if (daysAfter < 0) return { kind: 'releasedToday', version: release.version }
  if (daysAfter === 0) return { kind: 'onReleaseDay', version: release.version, share }
  return { kind: 'daysAfter', version: release.version, daysAfter, share }
}

/** The headline as one sentence. A release published today gets no share (no complete day on it yet). */
export function rolloutSentence(headline: RolloutHeadline): string {
  const v = `v${headline.version}`
  if (headline.kind === 'releasedToday') return `${v} came out today; its first full day is tomorrow`
  const lead = `${headline.share === null ? '–' : formatPercent(headline.share)} of active installs ran ${v} yesterday`
  if (headline.kind === 'onReleaseDay') return `${lead}, its release day`
  if (headline.kind === 'noReleaseDate') return lead
  return `${lead}, ${String(headline.daysAfter)} ${headline.daysAfter === 1 ? 'day' : 'days'} after its release`
}

/** Per-version daily counts for a stacked chart, oldest version first (the bottom layer). */
export interface VersionStack {
  days: string[]
  /** Every version with at least one install in `days`, oldest first. */
  versions: string[]
  /** Per version, one count per day in `days` (zero where it had none). */
  series: Record<string, number[]>
}

/** The rollout chart's layers over `days` (pass the view window's days). No "Older" bucket: every version draws. */
export function versionStack(days: ActiveInstallsDay[]): VersionStack {
  const versions = [
    ...new Set(days.flatMap((d) => Object.entries(d.byVersion).flatMap(([version, n]) => (n > 0 ? [version] : [])))),
  ].sort(compareVersions)
  const series: Record<string, number[]> = {}
  for (const version of versions) series[version] = days.map((d) => d.byVersion[version] ?? 0)
  return { days: days.map((d) => d.date), versions, series }
}
