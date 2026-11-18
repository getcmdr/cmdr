import { Hono } from 'hono'
import { verifyAdminAuth } from '../admin-auth'
import { osOfRow } from '../client-os'
import type { Bindings } from '../types'
import { beatSecondsSql } from './admin'
import { buildDateList, invalidDaysMessage, parseDays, shiftDate } from './funnel'

/**
 * `/admin/active-installs?days=N`: per UTC day, the heartbeat population (DAU, new installs, app hours,
 * rolling WAU and MAU, installs by version), plus distinct installs per version over the last 7, 30,
 * and 90 complete days. Install ids are folded into counts here and never leave the Worker.
 * `DETAILS.md` § Active installs.
 */

const activeInstalls = new Hono<{ Bindings: Bindings }>()

/** The dashboard's product page reads half a year, plus the 90-day comparison window before it. */
const defaultDays = 181

/** Coverage lengths, in days including the beat's own: an install is in WAU for a week from each beat. */
const wauDays = 7
const mauDays = 28

const windowLengths = [7, 30, 90] as const

export interface ActiveInstallsDay {
  /** UTC `YYYY-MM-DD`. */
  date: string
  /** Distinct installs that beat this day (the `/admin/heartbeat-dau` population). */
  dau: number
  /** Installs whose first-ever beat fell on this day (`install_summary.first_beat_at`, as `/admin/funnel`). */
  newInstalls: number
  /** The app's runtime this day, summed over installs, in hours (`beatSecondsSql`, as `/admin/heartbeat-dau`). */
  appHours: number
  /** Distinct installs with any beat in [day − 6, day]. */
  wau: number
  /** Distinct installs with any beat in [day − 27, day]. */
  mau: number
  /**
   * Installs by the version they ran this day: an install that beat on two versions counts under the
   * HIGHER one (`compareVersions`), so the values sum to `dau`.
   */
  byVersion: Record<string, number>
  /**
   * Installs by the OS they run (`macos`, `windows`, `linux`), summing to `dau`. A beat from a client older
   * than the `os` field is read from its `os_version` (`osOfRow`).
   */
  byOs: Record<string, number>
}

export interface VersionWindow {
  days: (typeof windowLengths)[number]
  /** First day of the window, UTC `YYYY-MM-DD`, inclusive. */
  since: string
  /** Last day of the window (yesterday), inclusive: the window holds complete days only. */
  until: string
  /** Distinct installs with any beat in the window. */
  installs: number
  /**
   * Distinct installs with any beat on each version in the window: the "installs exposed to v"
   * denominator. An install that upgraded counts under both versions, so the values can sum past
   * `installs` (unlike `ActiveInstallsDay.byVersion`).
   */
  byVersion: Record<string, number>
}

export interface ActiveInstallsResponse {
  /** Oldest first, the last N UTC days, today (partial) included. */
  days: ActiveInstallsDay[]
  /** 7, 30, and 90 days, each ending yesterday. */
  windows: VersionWindow[]
}

/** One beat from `activeInstallBeatsSql`: its UTC day, install, version, OS, and the runtime it accounts for. */
export interface BeatRow {
  date: string
  installId: string
  version: string
  /** The `os` the app sent; NULL from clients older than the field. */
  os: string | null
  osVersion: string
  seconds: number
}

export interface NewInstallRow {
  date: string
  count: number
}

/**
 * A total order on version strings, by numeric dot-separated parts. A part that isn't all digits (like
 * `0-beta` in `0.62.0-beta.1`) sorts below any number, so a pre-release sits below its release; two such
 * parts compare as strings, and a missing part sorts lowest. Equal keys fall back to a string compare,
 * so only identical strings are equal. ❌ Never string `MAX`: it puts `0.9.0` above `0.10.0`.
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

const dayMs = 86_400_000

function dayNumber(date: string): number {
  return Date.parse(`${date}T00:00:00Z`) / dayMs
}

/**
 * The first day the beat query must read for these output `dates`: 27 days before the first one (its
 * MAU), or 90 days before today (the 90-day window), whichever is earlier.
 */
export function readSinceFor(dates: string[]): string {
  const today = dates[dates.length - 1]
  return [shiftDate(dates[0], -(mauDays - 1)), shiftDate(today, -90)].sort()[0]
}

/**
 * For each output day, how many installs have a beat within `length` days up to and including it.
 * Per install, each beat covers [beat, beat + length − 1]; overlapping intervals merge, so an install
 * counts once per day, and every interval is clipped to the output range (a beat yesterday covers
 * today, never a day past it). A +1/−1 difference array then sums the coverage.
 */
function rollingDistinct(beatDaysByInstall: Map<string, Set<number>>, first: number, last: number, length: number) {
  const diff = new Array<number>(last - first + 2).fill(0)
  const add = (start: number, end: number) => {
    const from = Math.max(start, first)
    const to = Math.min(end, last)
    if (from > to) return
    diff[from - first] += 1
    diff[to - first + 1] -= 1
  }
  for (const beatDays of beatDaysByInstall.values()) {
    const sorted = [...beatDays].sort((x, y) => x - y)
    let start = sorted[0]
    let end = start + length - 1
    for (const beatDay of sorted.slice(1)) {
      if (beatDay <= end + 1) {
        end = beatDay + length - 1
      } else {
        add(start, end)
        start = beatDay
        end = beatDay + length - 1
      }
    }
    add(start, end)
  }
  const counts: number[] = []
  let running = 0
  for (let i = 0; i <= last - first; i++) {
    running += diff[i]
    counts.push(running)
  }
  return counts
}

function countBy(values: Iterable<string>): Record<string, number> {
  const counts: Record<string, number> = {}
  for (const value of values) counts[value] = (counts[value] ?? 0) + 1
  return counts
}

/**
 * Folds the beat rows into the response. `dates` are the output days, oldest
 * first, ending today; `rows` reach back to `readSinceFor(dates)`. Install ids stop here.
 */
export function assembleActiveInstalls(
  rows: BeatRow[],
  newRows: NewInstallRow[],
  dates: string[],
): ActiveInstallsResponse {
  const first = dayNumber(dates[0])
  const last = dayNumber(dates[dates.length - 1])
  const today = dates[dates.length - 1]

  const beatDaysByInstall = new Map<string, Set<number>>()
  /** Per output day: install → its highest version that day. */
  const versionsByDay = new Map<string, Map<string, string>>()
  /** Per output day: install → its OS. */
  const osesByDay = new Map<string, Map<string, string>>()
  const secondsByDay = new Map<string, number>()
  for (const row of rows) {
    const beatDay = dayNumber(row.date)
    const beatDays = beatDaysByInstall.get(row.installId) ?? new Set<number>()
    beatDays.add(beatDay)
    beatDaysByInstall.set(row.installId, beatDays)
    if (beatDay < first || beatDay > last) continue
    secondsByDay.set(row.date, (secondsByDay.get(row.date) ?? 0) + row.seconds)
    const versions = versionsByDay.get(row.date) ?? new Map<string, string>()
    const current = versions.get(row.installId)
    if (current === undefined || compareVersions(row.version, current) > 0) versions.set(row.installId, row.version)
    versionsByDay.set(row.date, versions)
    const oses = osesByDay.get(row.date) ?? new Map<string, string>()
    oses.set(row.installId, osOfRow(row))
    osesByDay.set(row.date, oses)
  }

  const newByDay = new Map(newRows.map((r) => [r.date, r.count]))
  const wau = rollingDistinct(beatDaysByInstall, first, last, wauDays)
  const mau = rollingDistinct(beatDaysByInstall, first, last, mauDays)
  const days = dates.map((date, i) => {
    const versions = versionsByDay.get(date) ?? new Map<string, string>()
    return {
      date,
      dau: versions.size,
      newInstalls: newByDay.get(date) ?? 0,
      appHours: (secondsByDay.get(date) ?? 0) / 3600,
      wau: wau[i],
      mau: mau[i],
      byVersion: countBy(versions.values()),
      byOs: countBy((osesByDay.get(date) ?? new Map<string, string>()).values()),
    }
  })

  const until = shiftDate(today, -1)
  const windows = windowLengths.map((length) => {
    const since = shiftDate(today, -length)
    const installs = new Set<string>()
    const byVersion = new Map<string, Set<string>>()
    for (const row of rows) {
      if (row.date < since || row.date > until) continue
      installs.add(row.installId)
      const exposed = byVersion.get(row.version) ?? new Set<string>()
      exposed.add(row.installId)
      byVersion.set(row.version, exposed)
    }
    return {
      days: length,
      since,
      until,
      installs: installs.size,
      byVersion: Object.fromEntries([...byVersion].map(([version, ids]) => [version, ids.size])),
    }
  })

  return { days, windows }
}

/**
 * One row per beat from `?1` (a `YYYY-MM-DD` day, which compares correctly against `created_at`'s
 * `YYYY-MM-DD HH:MM:SS`) on, with the runtime it accounts for: a range on `idx_heartbeat_created` that
 * reads each beat in the window once. ❌ No `GROUP BY date, anal_id, app_version` here: D1 bills the
 * grouping's sorter too, and on production it read 132,126 rows for 66,063 beats, twice this shape,
 * in 2.7x the SQL time (`DETAILS.md` § Active installs). ❌ No `MAX(app_version)` either: string order
 * puts `0.9.0` above `0.10.0`, so `assembleActiveInstalls` picks the highest version.
 */
export const activeInstallBeatsSql = `SELECT date(created_at) AS date, anal_id AS installId, app_version AS version,
       os, os_version AS osVersion, ${beatSecondsSql} AS seconds
  FROM heartbeat
 WHERE created_at >= ?1`

/** New installs per first-beat day, from `install_summary` (one row per install, `first_beat_at` indexed). */
export const newInstallsSql = `SELECT date(first_beat_at) AS date, COUNT(*) AS count
  FROM install_summary
 WHERE first_beat_at >= ?1
 GROUP BY date`

/** Runs both queries for the output `dates` (oldest first, ending today) and assembles the response. */
export async function queryActiveInstalls(db: D1Database, dates: string[]): Promise<ActiveInstallsResponse> {
  const [beats, installs] = await Promise.all([
    db.prepare(activeInstallBeatsSql).bind(readSinceFor(dates)).all<BeatRow>(),
    db.prepare(newInstallsSql).bind(dates[0]).all<NewInstallRow>(),
  ])
  return assembleActiveInstalls(beats.results, installs.results, dates)
}

activeInstalls.get('/admin/active-installs', async (c) => {
  const authError = await verifyAdminAuth(c)
  if (authError) return authError

  const days = parseDays(c.req.query('days'), defaultDays)
  if (days === null) return c.json({ error: invalidDaysMessage }, 400)

  const body = await queryActiveInstalls(c.env.TELEMETRY_DB, buildDateList(days, new Date()))
  return c.json(body)
})

export { activeInstalls }
