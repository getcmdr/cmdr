/**
 * Client-safe headline stats for the Product page (and the report), over the heartbeat's per-day active
 * installs. Same two rules as `acquisition-stats.ts`: today is partial and never read, and every rate pools
 * as a ratio of sums.
 *
 * Definitions (`apps/api-server/src/admin/active-installs.ts` counts them):
 * - DAU: distinct installs that beat that UTC day. WAU / MAU: distinct installs with a beat in the 7 / 28
 *   days ending that day (28 is four whole weeks, so the weekday mix never shifts it).
 * - Stickiness: Σ DAU ÷ Σ MAU over a window. Hours per active day: Σ app hours ÷ Σ DAU.
 *
 * Every tile reads yesterday and compares against the picker's N days:
 * - Point metrics (WAU, MAU, paying share) against their value N days earlier.
 * - Pooled metrics (stickiness, hours, D7) over the last N complete days (or final cohorts) against the N
 *   before; a window the data doesn't fully cover has no change.
 */
import type { ActiveInstallsDay } from './server/sources/active-installs.js'
import type { FunnelRow } from './server/sources/funnel.js'
import { addDays, ratioStats, type RatioPoint } from './acquisition-stats.js'
import { buildTrendInputs, d7End } from './acquisition-series.js'
import { paidLicensesThrough, type PaddleTransaction } from './money.js'

/** One headline tile. `null` is "couldn't say", never zero. */
export interface Tile {
  value: number | null
  /**
   * Versus the comparison window: relative for counts and hours (0.5 = +50%), absolute percentage points for
   * shares (0.01 = +1 pp). `null` when the comparison isn't fully covered.
   */
  change: number | null
}

const unknownTile: Tile = { value: null, change: null }

function valueOn<K extends keyof ActiveInstallsDay>(days: ActiveInstallsDay[], key: K, date: string) {
  return days.find((d) => d.date === date)?.[key] ?? null
}

/** A count read on yesterday, against the same count N days before. */
function pointTile(days: ActiveInstallsDay[], key: 'wau' | 'mau', n: number, today: string): Tile {
  const yesterday = addDays(today, -1)
  const value = valueOn(days, key, yesterday)
  const before = valueOn(days, key, addDays(yesterday, -n))
  return { value, change: value !== null && before ? value / before - 1 : null }
}

export function wauTile(days: ActiveInstallsDay[], n: number, today: string): Tile {
  return pointTile(days, 'wau', n, today)
}

export function mauTile(days: ActiveInstallsDay[], n: number, today: string): Tile {
  return pointTile(days, 'mau', n, today)
}

function ratioPoints(
  days: ActiveInstallsDay[],
  num: (d: ActiveInstallsDay) => number,
  den: (d: ActiveInstallsDay) => number,
): RatioPoint[] {
  return days.map((d) => ({ date: d.date, num: num(d), den: den(d) }))
}

/** Σ DAU ÷ Σ MAU over the last N complete days, change in percentage points. */
export function stickinessTile(days: ActiveInstallsDay[], n: number, today: string): Tile {
  const stats = ratioStats(
    ratioPoints(
      days,
      (d) => d.dau,
      (d) => d.mau,
    ),
    n,
    today,
  )
  return { value: stats.average, change: stats.change }
}

/** Σ app hours ÷ Σ DAU over the last N complete days, change relative (hours aren't a share). */
export function hoursPerActiveDayTile(days: ActiveInstallsDay[], n: number, today: string): Tile {
  const points = ratioPoints(
    days,
    (d) => d.appHours,
    (d) => d.dau,
  )
  const value = ratioStats(points, n, today).average
  const previous = ratioStats(points, n, addDays(today, -n)).average
  return { value, change: value !== null && previous ? value / previous - 1 : null }
}

/** D7 retention pooled over the last N final cohorts, change in percentage points. Null funnel: unknown. */
export function d7Tile(rows: FunnelRow[] | null, n: number, today: string): Tile {
  if (rows === null) return unknownTile
  const stats = ratioStats(buildTrendInputs(rows, today).d7, n, d7End(today))
  return { value: stats.average, change: stats.change }
}

/**
 * Paid licenses (first purchases, cumulative) ÷ MAU, on yesterday, against N days earlier, in percentage
 * points. A trend signal, not a conversion rate: refunded purchases still count, one license can cover
 * several Macs, and a licensed install with analytics off is in the numerator but not in MAU.
 */
export function payingShareTile(
  transactions: PaddleTransaction[],
  days: ActiveInstallsDay[],
  n: number,
  today: string,
): Tile {
  const share = (end: string): number | null => {
    const mau = valueOn(days, 'mau', addDays(end, -1))
    return mau ? paidLicensesThrough(transactions, end) / mau : null
  }
  const value = share(today)
  const before = share(addDays(today, -n))
  return { value, change: value !== null && before !== null ? value - before : null }
}
