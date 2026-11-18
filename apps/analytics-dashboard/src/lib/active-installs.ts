/**
 * Daily active installs and the update-check reach beside them.
 *
 * The opt-out rate is deliberately unmeasurable: an install that opts out of analytics sends nothing at
 * all, not even an "I opted out" bit, and that stays true. So the heartbeat's daily actives undercount by
 * an unknown amount. The Product page and the report print the real heartbeat numbers and show the reach
 * as context (the dotted line, `reachByDay`), never as a range around them:
 *
 * - **Daily actives**: distinct install ids that beat that day. Every one of these definitely ran the app.
 * - **Reach**: distinct IPs that checked for updates that day. Update checks ride a separate consent, so
 *   opted-out installs appear here.
 *
 * The reach is NOT an upper bound. IPs aren't installs: NAT collapses a household or an office to one, a
 * dynamic IP inflates one install across days, and an install with `updates.autoCheck` off never shows up
 * at all. It's the best evidence we have for how much the heartbeat misses, nothing more.
 */

import type { UpdateActivityRow } from './server/sources/cloudflare.js'
import { formatOs } from './format.js'

/** Distinct update-checking IPs per day, summed over the versions they checked from. */
function reachMap(updateActivity: UpdateActivityRow[]): Map<string, number> {
  const reach = new Map<string, number>()
  for (const row of updateActivity) reach.set(row.day, (reach.get(row.day) ?? 0) + row.updaters)
  return reach
}

/**
 * The reach on each of `dates`, for the dotted context line over the daily actives. A day the update data
 * doesn't cover is null (the line breaks there), never a zero.
 */
export function reachByDay(updateActivity: UpdateActivityRow[], dates: string[]): (number | null)[] {
  const reach = reachMap(updateActivity)
  return dates.map((date) => reach.get(date) ?? null)
}

/**
 * Each day's active installs as the two stacked layers of the daily chart: new (first beat that day) and
 * returning (the rest). Returning never goes below zero: the synthetic sweep can leave a just-counted new
 * install out of a later DAU.
 */
export function splitActives(days: { dau: number; newInstalls: number }[]): {
  newInstalls: number[]
  returning: number[]
} {
  return {
    newInstalls: days.map((d) => d.newInstalls),
    returning: days.map((d) => Math.max(0, d.dau - d.newInstalls)),
  }
}

/**
 * A day's active installs by OS, largest first, as the daily chart's tooltip lists them. Empty when the Worker
 * predates the split, so the tooltip shows nothing rather than a made-up "all macOS".
 */
export function installsByOs(byOs: Record<string, number> | undefined): { label: string; count: number }[] {
  return Object.entries(byOs ?? {})
    .map(([os, count]) => ({ label: formatOs(os), count }))
    .sort((a, b) => b.count - a.count || a.label.localeCompare(b.label))
}
