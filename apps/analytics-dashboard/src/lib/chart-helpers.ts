/**
 * Pure data-shaping helpers for the Download charts: stacking download rows by day, aggregating by a
 * field, and building per-day timelines. Kept out of the Svelte components so the section components
 * stay focused on markup. `DownloadRow` is a server type but the shape is plain data, safe to import
 * into client code.
 */
import type { DownloadRow } from '#lib/server/sources/cloudflare.js'

export interface StackSeries {
  key: string
  label: string
  color: string
  values: number[]
}

// Download source colors: website is the product gold, Homebrew an amber, everything else grey.
export const SOURCE_STACK = [
  { key: 'website', label: 'Website', color: '#ffc206' },
  { key: 'homebrew', label: 'Homebrew', color: '#f0883e' },
  { key: 'other', label: 'Direct / other', color: '#71717a' },
]

/** Aligns rows into a per-key map of per-day value arrays (one slot per entry in `days`). */
function stackByDay<T>(
  rows: T[],
  days: string[],
  getDay: (r: T) => string,
  getKey: (r: T) => string,
  getValue: (r: T) => number,
): Map<string, number[]> {
  const dayIndex = new Map(days.map((d, i) => [d, i]))
  const byKey = new Map<string, number[]>()
  for (const row of rows) {
    const di = dayIndex.get(getDay(row))
    if (di === undefined) continue
    const key = getKey(row)
    let arr = byKey.get(key)
    if (!arr) {
      arr = new Array<number>(days.length).fill(0)
      byKey.set(key, arr)
    }
    arr[di] += getValue(row)
  }
  return byKey
}

/** Downloads stacked by source, using the deduped same-day-distinct count. */
export function downloadSourceSeries(rows: DownloadRow[], days: string[]): StackSeries[] {
  const byKey = stackByDay(
    rows,
    days,
    (r) => r.day,
    (r) => r.source,
    (r) => r.uniqueDownloads,
  )
  return SOURCE_STACK.map((s) => ({
    ...s,
    values: byKey.get(s.key) ?? new Array<number>(days.length).fill(0),
  })).filter((s) => s.values.some((v) => v > 0))
}

/** Aggregates rows by a string field, summing a numeric field. */
export function aggregateBy(
  rows: DownloadRow[],
  groupField: keyof DownloadRow,
  sumField: keyof DownloadRow,
): Array<{ x: string; y: number }> {
  const map = new Map<string, number>()
  for (const row of rows) {
    const key = String(row[groupField])
    map.set(key, (map.get(key) ?? 0) + Number(row[sumField]))
  }
  return [...map.entries()].map(([x, y]) => ({ x, y })).sort((a, b) => b.y - a.y)
}

/** Returns sorted unique day strings and their unix timestamps from download rows. */
export function getDayAxis(rows: DownloadRow[]): { days: string[]; timestamps: number[] } {
  const days = [...new Set(rows.map((r) => r.day))].sort()
  const timestamps = days.map((d) => new Date(d).getTime() / 1000)
  return { days, timestamps }
}

/** Builds uPlot [timestamps[], values[]] for a filtered subset, aligned to the full day axis. */
export function buildTimeline(rows: DownloadRow[], allDays: string[], allTimestamps: number[]): [number[], number[]] {
  const byDay = new Map<string, number>()
  for (const row of rows) {
    byDay.set(row.day, (byDay.get(row.day) ?? 0) + row.downloads)
  }
  return [allTimestamps, allDays.map((d) => byDay.get(d) ?? 0)]
}

/** Compares two semver strings, descending (higher version first). */
export function compareSemverDesc(a: string, b: string): number {
  const pa = a.split('.').map(Number)
  const pb = b.split('.').map(Number)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const diff = (pb[i] ?? 0) - (pa[i] ?? 0)
    if (diff !== 0) return diff
  }
  return 0
}

/**
 * Finds the max daily download value across a set of groups, taken over every row's own day. Needs
 * no day axis: a day with no rows contributes nothing to a maximum.
 */
export function maxDailyAcrossGroups(rows: DownloadRow[], groupField: keyof DownloadRow, groupKeys: string[]): number {
  let max = 1
  for (const key of groupKeys) {
    const byDay = new Map<string, number>()
    for (const row of rows) {
      if (String(row[groupField]) === key) {
        byDay.set(row.day, (byDay.get(row.day) ?? 0) + row.downloads)
      }
    }
    for (const v of byDay.values()) {
      if (v > max) max = v
    }
  }
  return max
}
