/**
 * Renders the whole dashboard as one plain-text report: the agent- and David-readable dump behind
 * `GET /api/report`. Pure formatting, no fetching, so `format-report.test.ts` can pin the exact
 * output byte for byte. This file writes the funnel and the acquisition stages; `product-report.ts`
 * writes the Product page's half.
 *
 * The imports are relative rather than `#lib/...` on purpose: the vitest config doesn't load the
 * SvelteKit plugin, so the alias isn't resolvable from a plain unit test.
 */
import type { DashboardData } from '../../../lib/server/fetch-all.js'
import type { SourceResult } from '../../../lib/server/types.js'
import type { CloudflareData, DownloadRow } from '../../../lib/server/sources/cloudflare.js'
import type { UmamiData, UmamiMetricItem } from '../../../lib/server/sources/umami.js'
import type { GitHubData, GitHubRelease, GitHubStarsData } from '../../../lib/server/sources/github.js'
import type { PostHogData } from '../../../lib/server/sources/posthog.js'
import type { FunnelData, FunnelRow } from '../../../lib/server/sources/funnel.js'
import { aggregateChannels, aggregateReferers, aggregateUaFamilies } from '../../../lib/funnel.js'
import { compareVersions } from '../../../lib/rollout.js'
import { formatCountry, formatNumber as num } from '../../../lib/format.js'
import { ReportWriter, writeList } from './report-writer.js'
import { writeProductSections } from './product-report.js'

function pct(value: number, total: number): string {
  if (total === 0) return '0%'
  return `${((value / total) * 100).toFixed(1)}%`
}

function delta(current: number, previous: number): string {
  if (previous === 0) return ''
  const change = ((current - previous) / previous) * 100
  const sign = change >= 0 ? '+' : ''
  return ` (${sign}${change.toFixed(1)}% vs prior period)`
}

/** One ranked breakdown: a key and its total, biggest first (unless re-sorted by the caller). */
type Breakdown = Array<{ key: string; total: number }>

/** Aggregates download rows by a field, returning sorted [{key, total}] pairs. */
function aggregateBy(rows: DownloadRow[], field: keyof DownloadRow): Breakdown {
  const map = new Map<string, number>()
  for (const row of rows) {
    const key = String(row[field])
    map.set(key, (map.get(key) ?? 0) + row.downloads)
  }
  return [...map.entries()].map(([key, total]) => ({ key, total })).sort((a, b) => b.total - a.total)
}

/** A blank line, a heading, then one `  label: count (share%)` line per entry. Always written. */
function writeShares(
  w: ReportWriter,
  heading: string,
  entries: Breakdown,
  total: number,
  label: (key: string) => string,
  limit?: number,
): void {
  w.blank()
  w.line(heading)
  for (const entry of entries.slice(0, limit ?? entries.length)) {
    w.line(`  ${label(entry.key)}: ${num(entry.total)} (${pct(entry.total, total)})`)
  }
}

// ── 0. Daily funnel (always the last 30 UTC days, independent of the selected range) ──────────────

/** How many trailing days the report's funnel section shows. */
const reportFunnelDays = 30

function writeFunnelRows(w: ReportWriter, rows: FunnelRow[]): void {
  const dash = (n: number | null) => (n === null ? '-' : num(n))
  const pctOrDash = (f: number | null) => (f === null ? '-' : `${(f * 100).toFixed(0)}%`)
  w.line('day | visitors | dl clicks | server dls | installs | D7 | signups | purchases')
  // Most recent day first.
  for (const r of [...rows].reverse()) {
    w.line(
      `${r.date} | ${dash(r.visitors)} | ${dash(r.downloadClicks)} | ${dash(r.serverDownloads)} | ` +
        `${dash(r.newInstalls)} | ${pctOrDash(r.d7Retention)} | ${dash(r.newsletterSignups)} | ${dash(r.purchases)}`,
    )
  }
}

/**
 * First-touch ref rolled up over the whole 30-day window. "(none)" = no channel known (Homebrew,
 * direct links, return visits, cross-device journeys, and pre-2026-06-12 rows).
 */
function writeFunnelChannels(w: ReportWriter, rows: FunnelRow[]): void {
  const channels = aggregateChannels(rows)
  writeList(
    w,
    'Channels (last 30 days), downloads by first-touch ref:',
    channels,
    channels.length,
    (c) => `- ${c.ref}: ${num(c.count)}`,
  )
}

/**
 * The raw `Referer` host of each `/download` hit. Unlike the first-touch ref above (set only by the
 * website button), this is captured on every hit, so it reveals where the direct, no-ref downloads
 * came from. "(none)" = no usable referer (typed URL, privacy browser, referrer-policy strip,
 * Homebrew/curl, pre-2026-06-25 rows).
 */
function writeFunnelReferers(w: ReportWriter, rows: FunnelRow[]): void {
  const referers = aggregateReferers(rows)
  writeList(
    w,
    "Download referrers (last 30 days), by the /download hit's Referer host:",
    referers,
    referers.length,
    (r) => `- ${r.ref}: ${num(r.count)}`,
  )
}

/**
 * The `/download` User-Agent family. Cmdr is macOS-only, so a non-macOS UA can't be a real install.
 * "Human installs" drops only those provably-impossible bot hits and keeps the ambiguous ones, so the
 * headline stops reading as half noise. Note the scraper spoofs Mac browser UAs, so "human" is "could
 * be real", not proof; "unknown" (no/odd UA) stays counted, never excluded.
 */
function writeFunnelUaFamilies(w: ReportWriter, rows: FunnelRow[]): void {
  const ua = aggregateUaFamilies(rows)
  if (ua.total === 0) return
  w.blank()
  w.line('Downloads by client (last 30 days), by User-Agent family:')
  w.line(`- Human installs: ${num(ua.humanInstalls)} (of ${num(ua.total)} raw server downloads)`)
  w.line(`- human (Mac browser, Homebrew, or curl/wget): ${num(ua.human)}`)
  w.line(`- bot / impossible install (Windows, Android, Linux, or X11 UA, excluded): ${num(ua.bot)}`)
  w.line(`- unknown (no or unrecognized UA, kept in the count): ${num(ua.unknown)}`)
}

function writeFunnelSection(w: ReportWriter, funnel: SourceResult<FunnelData>): void {
  w.section('Daily funnel: the last 30 days, one row per UTC day')
  if (!funnel.ok) {
    w.line(`Couldn't load: ${funnel.error}`)
    return
  }
  // The source carries 180 days for the trend charts; the report keeps its 30-day table.
  const rows = funnel.data.rows.slice(-reportFunnelDays)
  writeFunnelRows(w, rows)
  writeFunnelChannels(w, rows)
  writeFunnelReferers(w, rows)
  writeFunnelUaFamilies(w, rows)
}

// ── 1. Awareness ─────────────────────────────────────────────────────────────────────────────────

function writeGitHubStars(w: ReportWriter, githubStars: SourceResult<GitHubStarsData>): void {
  if (!githubStars.ok) return
  const s = githubStars.data
  w.blank()
  w.line(`GitHub stars: ${num(s.totalStars)} total`)
  for (const repo of s.repos) {
    const recent7 = repo.daily
      .filter((d) => new Date(d.day) >= new Date(Date.now() - 7 * 86_400_000))
      .reduce((sum, d) => sum + d.newStars, 0)
    const recent30 = repo.daily
      .filter((d) => new Date(d.day) >= new Date(Date.now() - 30 * 86_400_000))
      .reduce((sum, d) => sum + d.newStars, 0)
    w.line(`  ${repo.repo}: ${num(repo.totalStars)} (last 7d: +${String(recent7)}, last 30d: +${String(recent30)})`)
  }
}

function writeTopReferrers(w: ReportWriter, heading: string, items: UmamiMetricItem[]): void {
  const total = items.reduce((s, r) => s + r.y, 0)
  writeList(w, heading, items, 15, (ref) => `  ${ref.x || '(direct)'}: ${num(ref.y)} (${pct(ref.y, total)})`)
}

function writeAwarenessSection(
  w: ReportWriter,
  umami: SourceResult<UmamiData>,
  githubStars: SourceResult<GitHubStarsData>,
): void {
  w.section('Awareness: how many people see Cmdr content?')
  if (!umami.ok) {
    w.line(`Couldn't load: ${umami.error}`)
    return
  }
  const u = umami.data
  const totalPv = u.personalSite.pageviews.value + u.website.pageviews.value + u.prvw.pageviews.value
  const prevPv = u.personalSite.pageviews.prev + u.website.pageviews.prev + u.prvw.pageviews.prev
  w.line(`- Total page views: ${num(totalPv)}${delta(totalPv, prevPv)}`)
  w.line(
    `- veszelovszki.com views: ${num(u.personalSite.pageviews.value)}${delta(u.personalSite.pageviews.value, u.personalSite.pageviews.prev)}`,
  )
  w.line(
    `- getcmdr.com views: ${num(u.website.pageviews.value)}${delta(u.website.pageviews.value, u.website.pageviews.prev)}`,
  )
  w.line(`- getprvw.com views: ${num(u.prvw.pageviews.value)}${delta(u.prvw.pageviews.value, u.prvw.pageviews.prev)}`)
  w.line(
    `- veszelovszki.com visitors: ${num(u.personalSite.visitors.value)}${delta(u.personalSite.visitors.value, u.personalSite.visitors.prev)}`,
  )
  w.line(
    `- getcmdr.com visitors: ${num(u.website.visitors.value)}${delta(u.website.visitors.value, u.website.visitors.prev)}`,
  )
  w.line(`- getprvw.com visitors: ${num(u.prvw.visitors.value)}${delta(u.prvw.visitors.value, u.prvw.visitors.prev)}`)

  writeGitHubStars(w, githubStars)
  writeTopReferrers(w, 'Top referrers (getcmdr.com):', u.websiteReferrers)
  writeTopReferrers(w, 'Top referrers (getprvw.com):', u.prvwReferrers)
  writeTopReferrers(w, 'Top referrers (veszelovszki.com):', u.personalSiteReferrers)
}

// ── 2. Interest ──────────────────────────────────────────────────────────────────────────────────

function writeInterestUmami(w: ReportWriter, u: UmamiData): void {
  w.line(
    `- getcmdr.com page views: ${num(u.website.pageviews.value)}${delta(u.website.pageviews.value, u.website.pageviews.prev)}`,
  )
  w.line(
    `- Unique visitors: ${num(u.website.visitors.value)}${delta(u.website.visitors.value, u.website.visitors.prev)}`,
  )
  w.line(
    `- Bounce rate: ${u.website.pageviews.value > 0 ? pct(u.website.bounces.value, u.website.visits.value) : 'N/A'}`,
  )

  writeList(w, 'Download button clicks:', u.downloadEvents, 10, (ev) => `  ${ev.x}: ${num(ev.y)}`)
  writeList(w, 'Top pages:', u.websitePages, 15, (page) => `  ${page.x}: ${num(page.y)} views`)
  const totalCountry = u.websiteCountries.reduce((s, c) => s + c.y, 0)
  writeList(
    w,
    'Website visitors by country:',
    u.websiteCountries,
    15,
    (c) => `  ${formatCountry(c.x)}: ${num(c.y)} (${pct(c.y, totalCountry)})`,
  )
}

function writeInterestSection(
  w: ReportWriter,
  umami: SourceResult<UmamiData>,
  posthog: SourceResult<PostHogData>,
): void {
  w.section('Interest: how many engage with the product page?')
  if (!umami.ok && !posthog.ok) {
    w.line(`Couldn't load: ${[umami.error, posthog.error].filter(Boolean).join('; ')}`)
    return
  }
  if (umami.ok) writeInterestUmami(w, umami.data)
  if (posthog.ok) {
    writeList(
      w,
      'Daily page views (PostHog):',
      posthog.data.dailyPageviews,
      posthog.data.dailyPageviews.length,
      (row) => `  ${row.day}: ${num(row.views)}`,
    )
  }
}

// ── 3. Download ──────────────────────────────────────────────────────────────────────────────────

/** One `  day: total` line per day, newest first, summing the given per-row field. */
function writeDailyTotals(
  w: ReportWriter,
  heading: string,
  rows: DownloadRow[],
  field: 'downloads' | 'uniqueDownloads',
): void {
  w.blank()
  w.line(heading)
  const byDay = new Map<string, number>()
  for (const row of rows) byDay.set(row.day, (byDay.get(row.day) ?? 0) + row[field])
  for (const [day, count] of [...byDay.entries()].sort(([a], [b]) => b.localeCompare(a))) {
    w.line(`  ${day}: ${num(count)}`)
  }
}

/** New installs by source (deduped). Keeps the order website, homebrew, other. */
function writeNewInstallsBySource(w: ReportWriter, rows: DownloadRow[], totalNew: number): void {
  const sourceOrder = ['website', 'homebrew', 'other']
  const newBySource = new Map<string, number>()
  for (const row of rows) newBySource.set(row.source, (newBySource.get(row.source) ?? 0) + row.uniqueDownloads)
  w.blank()
  w.line('New installs by source (deduped):')
  for (const key of sourceOrder) {
    const count = newBySource.get(key)
    if (count === undefined) continue
    w.line(`  ${key}: ${num(count)} (${pct(count, totalNew)})`)
  }
}

/** The top 10 countries crossed with a second dimension, as `  country: key: n, key: n`. */
function writeCountryCross(
  w: ReportWriter,
  heading: string,
  rows: DownloadRow[],
  byCountry: Breakdown,
  breakdown: (countryRows: DownloadRow[]) => Breakdown,
): void {
  w.blank()
  w.line(heading)
  for (const c of byCountry.slice(0, 10)) {
    const parts = breakdown(rows.filter((r) => r.country === c.key)).map((e) => `${e.key}: ${num(e.total)}`)
    w.line(`  ${formatCountry(c.key)}: ${parts.join(', ')}`)
  }
}

function writeDailyDownloadsByVersion(w: ReportWriter, rows: DownloadRow[], byVersion: Breakdown): void {
  w.blank()
  const topVersionKeys = byVersion.slice(0, 5).map((v) => v.key)
  w.line(`Daily downloads by version (top ${String(topVersionKeys.length)}):`)
  const days = [...new Set(rows.map((r) => r.day))].sort()
  for (const day of days) {
    const dayRows = rows.filter((r) => r.day === day)
    const parts = topVersionKeys
      .map((v) => {
        const count = dayRows.filter((r) => r.version === v).reduce((s, r) => s + r.downloads, 0)
        return count > 0 ? `${v}: ${String(count)}` : null
      })
      .filter((part) => part !== null)
    w.line(`  ${day}: ${parts.join(', ') || '(none)'}`)
  }
}

function writeDownloadBreakdowns(w: ReportWriter, rows: DownloadRow[], totalDl: number, totalNew: number): void {
  const byVersion = aggregateBy(rows, 'version').sort((a, b) => compareVersions(b.key, a.key))
  const byArch = aggregateBy(rows, 'arch')
  const byCountry = aggregateBy(rows, 'country')

  writeNewInstallsBySource(w, rows, totalNew)
  writeDailyTotals(w, 'Daily new installs (deduped):', rows, 'uniqueDownloads')
  writeShares(w, 'By version:', byVersion, totalDl, (key) => key)
  writeShares(w, 'By architecture:', byArch, totalDl, (key) => key)
  writeShares(w, 'By country:', byCountry, totalDl, formatCountry, 20)
  writeDailyTotals(w, 'Daily downloads:', rows, 'downloads')
  writeCountryCross(w, 'Top countries by architecture:', rows, byCountry, (countryRows) =>
    aggregateBy(countryRows, 'arch'),
  )
  writeCountryCross(w, 'Top countries by version:', rows, byCountry, (countryRows) =>
    aggregateBy(countryRows, 'version')
      .sort((a, b) => compareVersions(b.key, a.key))
      .slice(0, 5),
  )
  writeDailyDownloadsByVersion(w, rows, byVersion)
}

function writeCloudflareDownloads(w: ReportWriter, cf: CloudflareData, github: SourceResult<GitHubData>): void {
  const totalDl = cf.downloads.reduce((s, r) => s + r.downloads, 0)
  const totalNew = cf.downloads.reduce((s, r) => s + r.uniqueDownloads, 0)
  // Methodology, stated up front so the numbers below aren't a black box.
  w.line(
    '(New installs = DMG downloads via getcmdr.com, deduplicated to distinct people per day by daily-hashed IP, ' +
      'bot/link-preview hits dropped by user agent, in-app auto-updates excluded. Raw = every download request.)',
  )
  w.line(`- New installs (deduped): ${num(totalNew)}`)
  w.line(`- Download requests (raw): ${num(totalDl)}`)

  if (github.ok) {
    w.line(`- Downloads (GitHub, all-time): ${num(github.data.totalDownloads)}`)
  }

  if (cf.downloads.length > 0) writeDownloadBreakdowns(w, cf.downloads, totalDl, totalNew)
}

function writeGitHubReleases(w: ReportWriter, releases: GitHubRelease[]): void {
  writeList(
    w,
    'GitHub releases (all-time):',
    releases,
    10,
    (rel) => `  ${rel.tagName}: ${num(rel.totalDownloads)} downloads (published ${rel.publishedAt.split('T')[0]})`,
  )
}

function writeDownloadSection(
  w: ReportWriter,
  cloudflare: SourceResult<CloudflareData>,
  github: SourceResult<GitHubData>,
): void {
  w.section('Download: how many actually download?')
  if (!cloudflare.ok && !github.ok) {
    w.line(`Couldn't load: ${[cloudflare.error, github.error].filter(Boolean).join('; ')}`)
    return
  }
  if (cloudflare.ok) writeCloudflareDownloads(w, cloudflare.data, github)
  if (github.ok) writeGitHubReleases(w, github.data.releases)
}

// ── The report ───────────────────────────────────────────────────────────────────────────────────

/**
 * The whole report, section by section: the funnel and the acquisition stages first, then the Product
 * page's story in its order, ending with what users say back. Every section degrades to a "Couldn't
 * load" line on its own, so one dead source never blanks the rest.
 */
export function formatReport(data: DashboardData): string {
  const w = new ReportWriter()

  w.title(`Cmdr analytics report (${data.selection.range})`)
  w.line(`Generated: ${data.updatedAt}`)

  writeFunnelSection(w, data.funnel)
  writeAwarenessSection(w, data.umami, data.githubStars)
  writeInterestSection(w, data.umami, data.posthog)
  writeDownloadSection(w, data.cloudflare, data.github)
  writeProductSections(w, data)

  return w.text()
}
