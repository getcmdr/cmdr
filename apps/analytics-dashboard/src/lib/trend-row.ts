/**
 * Client-safe glue every `TrendRow` needs, shared by the Acquisition trends and the Product page's usage and
 * retention rows: the 90/30/7 stats per window, the daily and rolling rates (today's partial day gets no
 * rolling point), the slice of a full history that a chart shows, and the dash for an unknown cell.
 */
import {
  countStats,
  ratioStats,
  rollingRatio,
  statWindows,
  type CountPoint,
  type RatioPoint,
  type WindowStats,
} from './acquisition-stats.js'
import { rollingDays, type ViewWindow } from './time-range.js'
import { formatNumber } from './format.js'
import { safeRatio } from './acquisition-countries.js'

export type StatsByWindow = Record<(typeof statWindows)[number], WindowStats>

/** A count metric's stats grid: each window's complete days before `end` (today, so today stays out). */
export function countStatsByWindow(points: CountPoint[], end: string): StatsByWindow {
  return { 90: countStats(points, 90, end), 30: countStats(points, 30, end), 7: countStats(points, 7, end) }
}

/** A rate metric's stats grid, pooled per window. Pass a non-today `end` for cohorts (`d7End`). */
export function ratioStatsByWindow(points: RatioPoint[], end: string): StatsByWindow {
  return { 90: ratioStats(points, 90, end), 30: ratioStats(points, 30, end), 7: ratioStats(points, 7, end) }
}

/** Each day's own rate, or null where either side is unknown or the denominator is zero. */
export function dailyRatios(points: RatioPoint[]): (number | null)[] {
  return points.map((p) => safeRatio(p.num, p.den))
}

/** The rolling rate over complete days only: today (and anything after) gets no rolling point. */
export function rollingComplete(points: RatioPoint[], today: string): (number | null)[] {
  return rollingRatio(
    points.map((p) => (p.date >= today ? { ...p, num: null } : p)),
    rollingDays,
  )
}

/**
 * The days of `window` as indexes into a full history (oldest first, one entry per day): `start` is the
 * first drawn, `stop` one past the last. Slice every series with these, and index a day `i` of the chart as
 * `start + i`.
 */
export function viewRange(dates: string[], window: ViewWindow): { start: number; stop: number } {
  const first = dates.findIndex((d) => d >= window.start)
  const start = first === -1 ? dates.length : first
  return { start, stop: Math.max(start, dates.findLastIndex((d) => d <= window.end) + 1) }
}

/** A stat cell: the dash for unknown, never a zero. */
export function cell(value: number | null, format: (n: number) => string = formatNumber): string {
  return value === null ? '–' : format(value)
}
