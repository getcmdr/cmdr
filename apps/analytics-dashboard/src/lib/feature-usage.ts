/**
 * Client-safe types and views for per-feature usage, from the api-server's `/admin/feature-usage`.
 * The feature → event mapping is NOT here: it lives in one place, beside the SQL that reads it
 * (`apps/api-server/src/admin/feature-usage.ts`, `featureUsageDefinitions`). The dashboard only renders
 * what comes back, so a new mapped event needs no change on this side.
 */

import { formatPercent } from './format.js'

/** One slice of a feature's events: an event name, or a property value like an Ask Cmdr `outcome`. */
export interface FeatureUsagePart {
  key: string
  installs: number
  events: number
}

/** Mirrors `FeatureUsage` in `apps/api-server/src/admin/feature-usage.ts`. */
export interface FeatureUsage {
  /** A `feature-status.json` id. */
  id: string
  name: string
  /** The mapping in words, like `pane_navigated[volume_kind=archive]`. */
  sources: string[]
  installs: number
  events: number
  /** Whether any of the feature's events has ever been stored; `false` reads "no data yet". */
  everSeen: boolean
  /**
   * On a bounded (`?end=`) window only: the earliest stored event of the feature, RFC 3339, or null when none
   * was ever stored. A lower bound for a prop-filtered match.
   */
  firstSeenAt?: string | null
  parts: FeatureUsagePart[]
}

/** Mirrors `FeatureUsageResponse` in `apps/api-server/src/admin/feature-usage.ts`. */
export interface FeatureUsageData {
  range: string
  /** The window start, RFC 3339 UTC. */
  since: string
  /**
   * The EXCLUSIVE window end (midnight after the last day), RFC 3339 UTC, on a bounded window. Absent from
   * a Worker that predates `?end=`, whose window rolls up to now, today's partial hours included.
   */
  until?: string
  /** Installs that beat in the window: every share's denominator. */
  activeInstalls: number
  features: FeatureUsage[]
}

/** The label a feature with no stored events gets in place of a share. */
export const noDataYet = 'no data yet'

/**
 * The feature's installs as a share of the window's active installs, or "no data yet" when none of its
 * events has ever been stored (an event no release has shipped). A feature whose events exist but
 * nobody fired this window is a real 0%.
 */
export function formatFeatureShare(feature: FeatureUsage, activeInstalls: number): string {
  if (!feature.everSeen) return noDataYet
  const share = featureShare(feature, activeInstalls)
  return share === null ? '–' : formatPercent(share)
}

/** Most-used first (by installs, then events), keeping mapping order on ties; never-seen features last. */
export function sortFeatureUsage(features: FeatureUsage[]): FeatureUsage[] {
  return features
    .map((feature, index) => ({ feature, index }))
    .sort(
      (a, b) =>
        Number(b.feature.everSeen) - Number(a.feature.everSeen) ||
        b.feature.installs - a.feature.installs ||
        b.feature.events - a.feature.events ||
        a.index - b.index,
    )
    .map(({ feature }) => feature)
}

/** A feature's breakdown on one line, like "answered 2 (1 install), failed 1 (1 install)". */
export function formatParts(parts: FeatureUsagePart[]): string {
  return parts
    .map((part) => {
      const installs = `${part.installs.toLocaleString('en-US')} ${part.installs === 1 ? 'install' : 'installs'}`
      return `${part.key} ${part.events.toLocaleString('en-US')} (${installs})`
    })
    .join(', ')
}

/**
 * The page's two windows: the N complete days ending yesterday, and the N before. `previous` is null on a
 * Worker that predates `?end=` (then `current` is the legacy rolling window, today included: its window
 * line must say so, never "ending yesterday").
 */
export interface FeatureUsagePeriods {
  current: FeatureUsageData
  previous: FeatureUsageData | null
}

/** The feature's installs as a share of the window's active installs; null for "no data yet" or nobody active. */
export function featureShare(feature: FeatureUsage, activeInstalls: number): number | null {
  if (!feature.everSeen || activeInstalls === 0) return null
  return feature.installs / activeInstalls
}

/** The share bar's width, 0–1: the share itself, empty for "no data yet". */
export function shareBarWidth(feature: FeatureUsage, activeInstalls: number): number {
  return Math.min(1, featureShare(feature, activeInstalls) ?? 0)
}

/**
 * The share's change against the previous window, in percentage points (0.01 = +1 pp), or null when there's
 * no honest comparison: no previous window, the feature missing from it, no share on either side, or the
 * feature's first stored event falling after the previous window began (its zero there means "not stored
 * yet", not "nobody used it").
 */
export function shareChange(feature: FeatureUsage, periods: FeatureUsagePeriods): number | null {
  const { current, previous } = periods
  if (previous === null) return null
  if (feature.firstSeenAt === undefined || feature.firstSeenAt === null) return null
  if (Date.parse(feature.firstSeenAt) > Date.parse(previous.since)) return null
  const before = previous.features.find((f) => f.id === feature.id)
  if (before === undefined) return null
  const now = featureShare(feature, current.activeInstalls)
  const then = featureShare(before, previous.activeInstalls)
  return now !== null && then !== null ? now - then : null
}

/** Splits features into those with stored events and the "No data yet: a, b, c" ones, keeping order. */
export function foldNoData(features: FeatureUsage[]): { seen: FeatureUsage[]; noData: FeatureUsage[] } {
  return { seen: features.filter((f) => f.everSeen), noData: features.filter((f) => !f.everSeen) }
}
