/**
 * The report's Product half: the `/product` page's story in its order (active use, release rollout,
 * money, onboarding, feature usage, settings adoption, feedback and errors). Every number comes from the
 * pure helpers the page renders with (`product-stats`, `rollout`, `money`, `onboarding-funnel`, and
 * friends), never from math of its own, so the report and the page can't disagree. "Today" is the UTC
 * day of `updatedAt`: partial, so it's in no stat, only a marked line.
 *
 * Relative imports on purpose, as in `format-report.ts`: the vitest config doesn't resolve `#lib`.
 */
import type { DashboardData } from '../../../lib/server/fetch-all.js'
import type { SourceResult } from '../../../lib/server/types.js'
import type { CloudflareData } from '../../../lib/server/sources/cloudflare.js'
import type { PaddleData } from '../../../lib/server/sources/paddle.js'
import type { FunnelData } from '../../../lib/server/sources/funnel.js'
import type { ActiveInstallsDay } from '../../../lib/server/sources/active-installs.js'
import type { SettingAdoption, SettingsAdoption } from '../../../lib/server/settings-defaults.js'
import type { FeedbackAndErrorsData } from '../../../lib/server/sources/feedback-and-errors.js'
import type { FeedbackRow, ErrorReportRow } from '../../../lib/feedback-and-errors.js'
import {
  errorReportsByDay,
  errorsPer100,
  errorsSinceRelease,
  inboxFeedback,
  minInstallsForRate,
  tallyErrorReportsByField,
} from '../../../lib/feedback-and-errors.js'
import type { FeatureUsage, FeatureUsagePeriods } from '../../../lib/feature-usage.js'
import {
  formatFeatureShare,
  formatParts,
  noDataYet,
  shareChange,
  sortFeatureUsage,
} from '../../../lib/feature-usage.js'
import { reachByDay } from '../../../lib/active-installs.js'
import {
  formatShare,
  formatShareUnlike,
  formatValueShare,
  mostChanged,
  preferencesOnly,
  settingByKey,
  shareOnDefault,
  topMoved,
  topOverride,
  unchangedNote,
} from '../../../lib/settings-adoption.js'
import {
  analyticsOnTile,
  d7Tile,
  hoursPerActiveDayTile,
  mauTile,
  payingShareTile,
  stickinessTile,
  wauTile,
  type Tile,
} from '../../../lib/product-stats.js'
import {
  compareVersions,
  latestRelease,
  rolloutHeadline,
  rolloutSentence,
  type LatestRelease,
} from '../../../lib/rollout.js'
import {
  allTimeRevenue,
  paidLicensesThrough,
  recentPurchases,
  revenueInPeriod,
  type CurrencyAmount,
} from '../../../lib/money.js'
import {
  flagReach,
  minEligible,
  onboardingFunnel,
  upgradeNudgeFlag,
  type FlagReach,
} from '../../../lib/onboarding-funnel.js'
import {
  formatAverage,
  formatCurrency,
  formatNumber as num,
  formatPercent,
  formatPointChange,
  formatRelativeChange,
  formatUtcDateTime,
  type ChangeFormat,
} from '../../../lib/format.js'
import { rangeDays, shiftDay } from '../../../lib/time-range.js'
import { ReportWriter, writeList } from './report-writer.js'

// ── Shared inputs ────────────────────────────────────────────────────────────────────────────────

/** What every Product section reads besides its own source: the day, the range, and the release. */
interface ProductContext {
  /** The UTC day of `updatedAt`: partial, so no stat reads it. */
  today: string
  /** The picker's range in days: every "vs." comparison and every window. */
  n: number
  /** The heartbeat's per-day actives, or null when the source failed or the Worker lacks the endpoint. */
  days: ActiveInstallsDay[] | null
  /** The release the rollout and "since vX" lines track, as `/product` picks it. */
  release: LatestRelease | null
}

function productContext(data: DashboardData): ProductContext {
  const days = data.activeInstalls.ok ? (data.activeInstalls.data?.days ?? null) : null
  return {
    today: data.updatedAt.slice(0, 10),
    n: rangeDays[data.selection.range],
    days,
    release: latestRelease(data.github.ok ? data.github.data.releases : null, days ?? []),
  }
}

const notAvailableYet = "Not available yet: the api-server doesn't answer /admin/active-installs yet."

/** The days a range shows: its N complete days plus today, newest first. */
function windowNewestFirst(days: ActiveInstallsDay[], ctx: ProductContext): ActiveInstallsDay[] {
  const start = shiftDay(ctx.today, -ctx.n)
  return days.filter((d) => d.date >= start).reverse()
}

function dayLabel(date: string, ctx: ProductContext): string {
  return date === ctx.today ? `${date} (today, partial)` : date
}

/** One headline tile as `- Label: value (change comparison)`; a null reads `–`, never 0. */
function tileLine(
  label: string,
  tile: Tile,
  format: (value: number) => string,
  formatChange: ChangeFormat,
  comparison: string,
): string {
  const value = tile.value === null ? '–' : format(tile.value)
  const change = tile.change === null ? '–' : formatChange(tile.change)
  return `- ${label}: ${value} (${change} ${comparison})`
}

/** Amounts in one or more currencies, never added across them. */
function formatAmounts(amounts: CurrencyAmount[]): string {
  return amounts.length === 0 ? '0' : amounts.map((a) => formatCurrency(a.amount, a.currencyCode)).join(', ')
}

/** A flag's reach: the share with its counts, or the counts alone below `minEligible`. */
function formatReach(reach: FlagReach): string {
  const counts = `${num(reach.reached)} of ${num(reach.eligible)}`
  return reach.share === null ? `${counts} (too few to say)` : `${formatPercent(reach.share)} (${counts})`
}

// ── 4. Active use ────────────────────────────────────────────────────────────────────────────────

function analyticsOnLine(
  days: ActiveInstallsDay[],
  cloudflare: SourceResult<CloudflareData>,
  ctx: ProductContext,
): string {
  if (!cloudflare.ok) return `- Analytics on: couldn't load the update checks (${cloudflare.error})`
  const tile = analyticsOnTile(days, cloudflare.data.updateActivity, ctx.n, ctx.today)
  return tileLine('Analytics on', tile, formatPercent, formatPointChange, `vs. previous ${String(ctx.n)} days`)
}

function writeActiveUseTiles(
  w: ReportWriter,
  days: ActiveInstallsDay[],
  cloudflare: SourceResult<CloudflareData>,
  ctx: ProductContext,
): void {
  const earlier = `vs. ${String(ctx.n)} days earlier`
  const previous = `vs. previous ${String(ctx.n)} days`
  w.line(tileLine('Weekly active', wauTile(days, ctx.n, ctx.today), num, formatRelativeChange, earlier))
  w.line(tileLine('Monthly active', mauTile(days, ctx.n, ctx.today), num, formatRelativeChange, earlier))
  w.line(tileLine('Stickiness', stickinessTile(days, ctx.n, ctx.today), formatPercent, formatPointChange, previous))
  w.line(
    tileLine(
      'Hours per active day',
      hoursPerActiveDayTile(days, ctx.n, ctx.today),
      (v) => `${formatAverage(v)} h`,
      formatRelativeChange,
      previous,
    ),
  )
  w.line(analyticsOnLine(days, cloudflare, ctx))
}

/** One line per day of the range, newest first, with the update-check reach as context. */
function writeDailyActives(
  w: ReportWriter,
  days: ActiveInstallsDay[],
  cloudflare: SourceResult<CloudflareData>,
  ctx: ProductContext,
): void {
  const shown = windowNewestFirst(days, ctx)
  const reach = cloudflare.ok
    ? reachByDay(
        cloudflare.data.updateActivity,
        shown.map((d) => d.date),
      )
    : null
  const lines = shown.map((d, i) => {
    const r = reach?.[i] ?? null
    const checked = r === null ? 'no update-check data' : `${num(r)} checked for updates`
    return `  ${dayLabel(d.date, ctx)}: ${num(d.dau)} active, ${num(d.newInstalls)} new, ${num(Math.round(d.appHours))} app hours, ${checked}`
  })
  writeList(w, 'Daily active installs (newest first):', lines, lines.length, (line) => line)
}

/** D7 from the funnel, which loads on its own: a heartbeat outage doesn't take it down. */
function d7Line(funnel: SourceResult<FunnelData>, ctx: ProductContext): string {
  if (!funnel.ok) return `- D7 retention: couldn't load (${funnel.error})`
  // `unavailable` is the funnel's worker call failing (`FunnelData.workerStatus`), as the page's tile reads it.
  if (funnel.data.workerStatus === 'unavailable')
    return "- D7 retention: couldn't load (the api-server's funnel didn't answer)"
  const tile = d7Tile(funnel.data.rows, ctx.n, ctx.today)
  return tileLine('D7 retention', tile, formatPercent, formatPointChange, `vs. previous ${String(ctx.n)} cohorts`)
}

/**
 * The heartbeat's real numbers, read on yesterday, with the update-check reach as context beside each
 * day (David's call 2026-11-17: no more floor–reach range).
 */
function writeActiveUseSection(w: ReportWriter, data: DashboardData, ctx: ProductContext): void {
  w.section('Active use: how many run the app?')
  const n = String(ctx.n)
  w.line(`Yesterday, ${shiftDay(ctx.today, -1)} (UTC), the last complete day. Today is partial and in no number here.`)
  if (!data.activeInstalls.ok) w.line(`- Couldn't load the heartbeat: ${data.activeInstalls.error}`)
  else if (ctx.days === null) w.line(`- ${notAvailableYet}`)
  else writeActiveUseTiles(w, ctx.days, data.cloudflare, ctx)
  w.line(d7Line(data.funnel, ctx))
  if (ctx.days === null) return

  w.blank()
  w.line('Counts installs with usage stats on; an install that opted out sends nothing. Weekly and monthly')
  w.line(`active count installs with a beat in the 7 and 28 days ending yesterday, against the same count ${n} days`)
  w.line('earlier. Stickiness is daily active over monthly active, and hours per active day is all app hours over')
  w.line(`all daily actives, each pooled over the last ${n} complete days against the ${n} before. A beat from a build`)
  w.line(`too old to report its runtime counts as an hour. D7 pools the newest ${n} cohorts old enough to have one.`)
  w.line('The last week can still hold a few synthetic installs the weekly sweep removes later.')
  w.line('"Checked for updates" is context, not a ceiling: distinct addresses that checked for updates, a separate')
  w.line('consent installs with analytics off still ride. A shared connection counts once, a changing address')
  w.line('counts more than once, and anyone with automatic update checks off never appears.')
  w.line(`"Analytics on" is daily active over those addresses, pooled over the last ${n} complete days against the`)
  w.line(`${n} before. Shared connections push it up, a laptop on several networks or an install updating mid-day`)
  w.line('pushes it down, and auto-check off is negligible: read it as a rough share, about ±10 points.')
  writeDailyActives(w, ctx.days, data.cloudflare, ctx)
}

// ── 4b. Release rollout ──────────────────────────────────────────────────────────────────────────

/** Each day's installs by the highest version they ran, newest version first: the page's rollout chart. */
function writeVersionsByDay(w: ReportWriter, days: ActiveInstallsDay[], ctx: ProductContext): void {
  const lines = windowNewestFirst(days, ctx).map((d) => {
    const parts = Object.entries(d.byVersion)
      .filter(([, count]) => count > 0)
      .sort(([a], [b]) => compareVersions(b, a))
      .map(([version, count]) => `v${version}: ${num(count)}`)
    return `  ${dayLabel(d.date, ctx)}: ${num(d.dau)} total${parts.length > 0 ? ` (${parts.join(', ')})` : ''}`
  })
  writeList(
    w,
    'Installs by version per day (each install at the highest version it ran that day; newest first):',
    lines,
    lines.length,
    (line) => line,
  )
}

function writeRolloutSection(w: ReportWriter, data: DashboardData, ctx: ProductContext): void {
  w.section('Release rollout: how far does the newest release spread?')
  if (!data.activeInstalls.ok) {
    w.line(`Couldn't load: ${data.activeInstalls.error}`)
    return
  }
  if (ctx.days === null) {
    w.line(`- ${notAvailableYet}`)
    return
  }
  w.line(
    ctx.release === null
      ? '- No release to track yet.'
      : `- ${rolloutSentence(rolloutHeadline(ctx.release, ctx.days, ctx.today))}`,
  )
  writeVersionsByDay(w, ctx.days, ctx)
}

// ── 5. Money ─────────────────────────────────────────────────────────────────────────────────────

function writeMoneyFigures(w: ReportWriter, data: DashboardData, paddle: PaddleData, ctx: ProductContext): void {
  const txns = paddle.allTransactions
  const vsPrevious = `vs. previous ${String(ctx.n)} days`
  const period = revenueInPeriod(txns, ctx.n, ctx.today)
    .map(
      (line) =>
        `${formatCurrency(line.amount, line.currencyCode)} (${line.change === null ? '–' : formatRelativeChange(line.change)} ${vsPrevious})`,
    )
    .join(', ')
  w.line(`- Revenue, last ${String(ctx.n)} complete days: ${period || '0'}`)
  w.line(`- All-time revenue: ${formatAmounts(allTimeRevenue(txns, ctx.today))}`)
  w.line(`- Paid licenses (first purchases through yesterday): ${num(paidLicensesThrough(txns, ctx.today))}`)
  const paying = ctx.days ? payingShareTile(txns, ctx.days, ctx.n, ctx.today) : { value: null, change: null }
  w.line(
    tileLine(
      'Paying share of monthly active',
      paying,
      formatPercent,
      formatPointChange,
      `vs. ${String(ctx.n)} days earlier`,
    ),
  )

  const license = data.license
  if (license.ok) {
    const devices = license.data.activeDevices === null ? '–' : num(license.data.activeDevices)
    w.line(`- License activations: ${num(license.data.totalActivations)} (${devices} active devices)`)
  } else {
    w.line(`- License activations: couldn't load (${license.error})`)
  }

  const settings = data.settingsAdoption
  const nudge = settings.ok ? flagReach(settings.data.settings, upgradeNudgeFlag) : null
  const nudgeText = !settings.ok
    ? `couldn't load (${settings.error})`
    : nudge === null
      ? 'not available yet'
      : formatReach(nudge)
  w.line(`- ${upgradeNudgeFlag.label}: ${nudgeText}`)
}

function writeMoneySection(w: ReportWriter, data: DashboardData, ctx: ProductContext): void {
  w.section('Money: who pays?')
  if (!data.paddle.ok) {
    w.line(`Couldn't load: ${data.paddle.error}`)
    return
  }
  writeMoneyFigures(w, data, data.paddle.data, ctx)
  w.blank()
  w.line('Revenue counts complete days and stays per currency: amounts in two currencies are never added. A')
  w.line("transaction counts in its payout currency when Paddle reports one, else its checkout currency. It's gross")
  w.line('of refunds: a refunded purchase stays completed in Paddle. Purchases are first purchases (checkout or API);')
  w.line("renewals, card updates, and plan changes aren't. Paying share is a trend signal, not a conversion rate:")
  w.line('refunded purchases still count, one license can cover several Macs, and a licensed install with analytics')
  w.line('off is paid but not in monthly active.')

  const purchases = recentPurchases(data.paddle.data.allTransactions, ctx.n, ctx.today)
  writeList(
    w,
    `Purchases, last ${String(ctx.n)} days (first purchases, newest first):`,
    purchases,
    purchases.length,
    (p) =>
      `  ${formatUtcDateTime(p.createdAt)} UTC${p.isToday ? ' (today)' : ''}: ${formatCurrency(p.amount.amount, p.amount.currencyCode)}`,
  )
}

// ── 6. Onboarding ────────────────────────────────────────────────────────────────────────────────

function writeOnboardingSection(w: ReportWriter, settingsAdoption: SourceResult<SettingsAdoption>): void {
  w.section('Onboarding: how far do installs get?')
  if (!settingsAdoption.ok) {
    w.line(`Couldn't load: ${settingsAdoption.error}`)
    return
  }
  const funnel = onboardingFunnel(settingsAdoption.data.settings)
  w.line(
    `- ${funnel.step?.label ?? 'Finished onboarding'}: ${funnel.step ? formatReach(funnel.step) : 'not available yet'}`,
  )
  const row = (reach: FlagReach) =>
    `  ${reach.label}${reach.capabilityGated ? '*' : ''} (${reach.key}): ${formatReach(reach)}`
  writeList(w, 'Discovered (highest share first):', funnel.discovered, funnel.discovered.length, row)
  writeList(w, 'Hit a snag:', funnel.friction, funnel.friction.length, row)
  writeList(w, 'Closed by hand:', funnel.dismissed, funnel.dismissed.length, row)
  w.blank()
  w.line('Each install counts once, at its latest heartbeat; a flag off its default means the install saw, did, or')
  w.line(
    `closed that thing. Each share is over the installs whose build has the flag. Under ${String(minEligible)} such installs a row`,
  )
  w.line('shows the count only. A * marks a hint that only shows where macOS has the feature behind it, so its share')
  w.line('also counts installs that could never see it.')
}

// ── 7. Feature usage ─────────────────────────────────────────────────────────────────────────────

function writeFeatureUsageRow(w: ReportWriter, feature: FeatureUsage, periods: FeatureUsagePeriods, n: number): void {
  const label = `  ${feature.id} (${feature.name})`
  if (!feature.everSeen) {
    w.line(`${label}: ${noDataYet}`)
    return
  }
  const activeInstalls = periods.current.activeInstalls
  const change = shareChange(feature, periods)
  const installs = `${num(feature.installs)} ${feature.installs === 1 ? 'install' : 'installs'}`
  const events = `${num(feature.events)} ${feature.events === 1 ? 'event' : 'events'}`
  const share = `${formatFeatureShare(feature, activeInstalls)}, ${change === null ? '–' : formatPointChange(change)} vs. previous ${String(n)} days`
  const head = `${label}: ${installs} (${share}), ${events}`
  w.line(feature.parts.length > 0 ? `${head}; ${formatParts(feature.parts)}` : head)
}

/** The window line: complete days ending yesterday, or an older Worker's rolling window (today included). */
function featureWindowLine(periods: FeatureUsagePeriods): string {
  const { since, until } = periods.current
  if (until === undefined)
    return `- Window: since ${since.slice(0, 10)}, today included (the api-server predates complete-day windows)`
  return `- Window: ${since.slice(0, 10)} – ${shiftDay(until.slice(0, 10), -1)}, complete days`
}

/**
 * Per-feature usage for the feature-status review: which features people reach for, against the
 * window's active installs. The mapping is the api-server's (`featureUsageDefinitions`), listed at the
 * end so a reader can check what each row counts.
 */
function writeFeatureUsageSection(
  w: ReportWriter,
  featureUsage: SourceResult<FeatureUsagePeriods>,
  ctx: ProductContext,
): void {
  w.section('Feature usage: which features do people use?')
  if (!featureUsage.ok) {
    w.line(`Couldn't load: ${featureUsage.error}`)
    return
  }
  const periods = featureUsage.data
  const current = periods.current
  w.line(featureWindowLine(periods))
  w.line(`- Active installs (beat at least once in the window): ${num(current.activeInstalls)}`)
  w.blank()
  w.line(
    "An install counts toward a feature once it fired any of that feature's events in the window; the share " +
      'is of active installs. "No data yet" means no release has sent the event yet, so it isn\'t a zero. The ' +
      'change is in percentage points against the window before, and a dash when there is no honest comparison ' +
      "(the feature's events weren't stored yet then).",
  )
  w.blank()
  w.line('Per feature (id from feature-status.json, most-used first):')
  for (const feature of sortFeatureUsage(current.features)) writeFeatureUsageRow(w, feature, periods, ctx.n)
  writeList(w, 'Counted from:', current.features, current.features.length, (f) => `  ${f.id}: ${f.sources.join(', ')}`)
}

// ── 8. Settings adoption ─────────────────────────────────────────────────────────────────────────

/** How many moved settings the report lists, as the page's table leads with. */
const settingsShown = 15

/** One line per preference somebody has moved, the most-moved first. */
function writeChangedSettings(w: ReportWriter, preferences: SettingAdoption[]): void {
  const changed = mostChanged(preferences)
  w.blank()
  if (changed.length === 0) {
    w.line('Nobody has moved a setting off its default yet.')
    return
  }
  w.line(`Settings people change (most-moved first; the other ${unchangedNote(preferences.length - changed.length)}):`)
  for (const setting of topMoved(preferences, settingsShown)) {
    const share = shareOnDefault(setting)
    const override = topOverride(setting)
    const movedIn =
      setting.defaultLabel === null && setting.defaultMovedIn ? ` (moved in v${setting.defaultMovedIn})` : ''
    const parts = [
      `default ${setting.defaultLabel ?? setting.latestDefaultLabel}${movedIn}`,
      `${num(setting.eligible)} installs`,
      `${share === null ? 'n/a' : formatShare(share)} on default`,
    ]
    if (override) parts.push(`most common change: ${override.label} (${num(override.installs)})`)
    w.line(`  ${setting.key}: ${parts.join(', ')}`)
  }
  if (changed.length > settingsShown) w.line(`  …and ${num(changed.length - settingsShown)} more moved settings`)
}

/**
 * Adoption, which the raw config shape can't give: the app saves only settings someone changed, so
 * an absent key is resolved against the defaults that shipped in that install's version. Preferences
 * only: remembered UI and onboarding state (`kind: 'state'`) belong to the Onboarding section.
 */
function writeSettingsAdoptionSection(w: ReportWriter, settingsAdoption: SourceResult<SettingsAdoption>): void {
  w.section('Settings adoption: what do people actually turn on?')
  if (!settingsAdoption.ok) {
    w.line(`Couldn't load: ${settingsAdoption.error}`)
    return
  }
  const data = settingsAdoption.data
  const readable = data.totalInstalls - data.unresolvedInstalls
  if (readable === 0) {
    w.line('- No installs to read yet (settings adoption fills as beta testers run a build that reports a config).')
    return
  }

  w.line(`- Installs we can read: ${num(readable)}`)
  if (data.unresolvedInstalls > 0) {
    w.line(`- Not counted: ${num(data.unresolvedInstalls)} on a version older than the settings history goes back`)
  }
  w.line(`- Drive indexing on: ${formatValueShare(settingByKey(data.settings, 'indexing.enabled'), 'on')}`)
  w.line(`- Image search on: ${formatValueShare(settingByKey(data.settings, 'mediaIndex.enabled'), 'on')}`)
  w.line(`- AI switched on: ${formatShareUnlike(settingByKey(data.settings, 'ai.provider'), 'off')}`)
  w.blank()
  w.line(
    'Each install counts once, at its latest heartbeat. A setting is scored only against installs whose build ' +
      'actually had it, so a young setting shows a smaller total than an old one. Only preferences show here; ' +
      'remembered UI and onboarding state are in the Onboarding section.',
  )
  writeChangedSettings(w, preferencesOnly(data.settings))
}

// ── 9. Feedback & errors ─────────────────────────────────────────────────────────────────────────

/**
 * Error reports per 100 installs by version over the range's complete days, as the page's inbox shows
 * them; without the heartbeat's version window, the plain counts by version.
 */
function writeErrorsByVersion(
  w: ReportWriter,
  fe: FeedbackAndErrorsData,
  data: DashboardData,
  ctx: ProductContext,
): void {
  const window = data.activeInstalls.ok ? data.activeInstalls.data?.windows.find((v) => v.days === ctx.n) : undefined
  if (window === undefined) {
    const byVersion = tallyErrorReportsByField(fe.errorReports, 'appVersion')
    writeList(w, 'Error reports by version:', byVersion, byVersion.length, (v) => `  ${v.key}: ${num(v.count)}`)
    return
  }
  const rates = errorsPer100(fe.allErrorReports, window).filter((r) => r.reports > 0)
  w.blank()
  w.line(
    `Error reports per 100 installs, by version (${window.since} – ${window.until}, complete days; an install ` +
      `that upgraded counts under both; under ${String(minInstallsForRate)} installs, no rate):`,
  )
  if (rates.length === 0) w.line('  No error reports on complete days in this range.')
  for (const r of rates) {
    const rate = r.per100 === null ? 'too few installs for a rate' : `${formatAverage(r.per100)} per 100`
    w.line(
      `  ${r.version}: ${num(r.reports)} ${r.reports === 1 ? 'report' : 'reports'}, ${num(r.installs)} installs, ${rate}`,
    )
  }
}

function writeErrorReportBreakdowns(
  w: ReportWriter,
  fe: FeedbackAndErrorsData,
  data: DashboardData,
  ctx: ProductContext,
): void {
  if (fe.errorReports.length === 0) return
  const byKind = tallyErrorReportsByField(fe.errorReports, 'kind')
  writeList(w, 'Error reports by kind:', byKind, byKind.length, (k) => `  ${k.key}: ${num(k.count)}`)
  writeErrorsByVersion(w, fe, data, ctx)
  const byDay = errorReportsByDay(fe.errorReports)
  writeList(w, 'Error reports by day:', byDay, byDay.length, (d) => `  ${d.date}: ${num(d.count)}`)
}

function writeRecentFeedback(w: ReportWriter, feedback: FeedbackRow[]): void {
  writeList(w, 'Recent feedback, and every message awaiting a reply:', feedback, feedback.length, (msg) => {
    const answered = msg.answeredAt ? `, answered ${msg.answeredAt.split(' ')[0]}` : ''
    const replyTo = msg.email ? ` [reply-to: ${msg.email}${answered}]` : ''
    const logs = msg.errorReportId ? ` [logs: ${msg.errorReportId}]` : ''
    const text = msg.feedback.replace(/\s+/g, ' ').slice(0, 280)
    return `  ${msg.createdAt.split(' ')[0]} (v${msg.appVersion})${replyTo}${logs}: ${text}`
  })
}

/** Error reports since the tracked release came out, whatever the range (the page's top strip). */
function sinceReleaseLine(reports: ErrorReportRow[], release: LatestRelease | null): string | null {
  if (release?.publishedAt == null) return null
  return `- Error reports since v${release.version} came out: ${num(errorsSinceRelease(reports, release.publishedAt))}`
}

function writeFeedbackSection(w: ReportWriter, data: DashboardData, ctx: ProductContext): void {
  w.section('Feedback & errors: what are users telling us?')
  const feedbackAndErrors = data.feedbackAndErrors
  if (!feedbackAndErrors.ok) {
    w.line(`Couldn't load: ${feedbackAndErrors.error}`)
    return
  }
  const fe = feedbackAndErrors.data
  w.line(`- Feedback messages: ${num(fe.feedback.length)}`)
  w.line(`- Awaiting reply (a reply-to email, not marked answered, however old): ${num(fe.awaitingReply.length)}`)
  w.line(`- Error reports: ${num(fe.errorReports.length)}`)
  const sinceRelease = sinceReleaseLine(fe.allErrorReports, ctx.release)
  if (sinceRelease !== null) w.line(sinceRelease)
  writeErrorReportBreakdowns(w, fe, data, ctx)
  writeRecentFeedback(w, inboxFeedback(fe.feedback, fe.awaitingReply, 30))
}

/** The Product half, section by section in the page's order. Each degrades to its own "Couldn't load". */
export function writeProductSections(w: ReportWriter, data: DashboardData): void {
  const ctx = productContext(data)
  writeActiveUseSection(w, data, ctx)
  writeRolloutSection(w, data, ctx)
  writeMoneySection(w, data, ctx)
  writeOnboardingSection(w, data.settingsAdoption)
  writeFeatureUsageSection(w, data.featureUsage, ctx)
  writeSettingsAdoptionSection(w, data.settingsAdoption)
  writeFeedbackSection(w, data, ctx)
}
