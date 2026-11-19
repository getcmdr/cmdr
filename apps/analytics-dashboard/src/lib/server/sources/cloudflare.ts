import type { DashboardSelection, SourceResult, TimeRange } from '../types.js'
import { rangeDays, selectionCacheKey } from '../types.js'
import { cacheGet, cacheSet } from '../cache.js'
import { fetchWorkerEndpoint } from './worker-endpoint.js'

export interface DownloadRow {
  version: string
  /** The file's OS: `macos`, `windows`, or `linux`. `macos` for every row from a Worker older than the field. */
  os: string
  arch: string
  country: string
  /** 'website' | 'homebrew' | 'other'. See the `/download` handler in the api-server. */
  source: string
  day: string
  /** Raw download requests (one row per hit). */
  downloads: number
  /** Distinct same-day downloaders (deduped by daily-salted hashed IP). Always <= `downloads`. */
  uniqueDownloads: number
}

/** One day of update-check activity: distinct update-enabled installs that ran while on `version`. */
export interface UpdateActivityRow {
  day: string
  version: string
  updaters: number
}

export interface CloudflareData {
  downloads: DownloadRow[]
  updateActivity: UpdateActivityRow[]
}

interface CloudflareEnv {
  LICENSE_SERVER_ADMIN_TOKEN: string
  /** Optional override for the api-server base URL (local QA). Defaults to production. */
  WORKER_BASE_URL?: string
}

/**
 * The downloads/update-activity range param. Those two endpoints take 24h/7d/30d/all (no 90d), so 90d asks
 * for `all` and `sinceDay` trims the rows to the window.
 */
const downloadRangeMap: Record<TimeRange, string> = {
  '7d': '7d',
  '30d': '30d',
  '90d': 'all',
}

/**
 * The update-activity range param: wide enough for two ranges of complete days plus today, so the "Analytics
 * on" tile has its previous window (`updateActivitySince` trims the rest).
 */
const updateRangeMap: Record<TimeRange, string> = {
  '7d': '30d',
  '30d': 'all',
  '90d': 'all',
}

function utcDayMinus(now: Date, days: number): string {
  const today = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate())
  return new Date(today - days * 86_400_000).toISOString().slice(0, 10)
}

/** The first UTC day a range covers (`days` days ending today), for trimming an `all` answer. */
export function sinceDay(range: TimeRange, now: Date): string {
  return utcDayMinus(now, rangeDays[range] - 1)
}

/** The first UTC day of update activity kept: two ranges of complete days before today. */
export function updateActivitySince(range: TimeRange, now: Date): string {
  return utcDayMinus(now, 2 * rangeDays[range])
}

interface WorkerDownloadRow {
  date: string
  version: string
  /** Absent from a Worker older than migration 0033, which only ever served the macOS DMG. */
  os?: string
  arch: string
  country: string
  source: string
  count: number
  uniqueCount: number
}

interface WorkerUpdateActivityRow {
  date: string
  version: string
  count: number
}

export function parseDownloadRows(raw: WorkerDownloadRow[]): DownloadRow[] {
  return raw.map((row) => ({
    version: row.version,
    os: row.os ?? 'macos',
    arch: row.arch,
    country: row.country,
    source: row.source,
    day: row.date,
    downloads: row.count,
    uniqueDownloads: row.uniqueCount,
  }))
}

export function parseUpdateActivityRows(raw: WorkerUpdateActivityRow[]): UpdateActivityRow[] {
  return raw.map((row) => ({ day: row.date, version: row.version, updaters: row.count }))
}

/** v2: `updateActivity` reaches back two ranges (the "Analytics on" tile's previous window). */
const cacheSource = 'cloudflare-v2'

export async function fetchCloudflareData(
  env: CloudflareEnv,
  selection: DashboardSelection,
): Promise<SourceResult<CloudflareData>> {
  const cached = await cacheGet<CloudflareData>(cacheSource, selectionCacheKey(selection))
  if (cached) return { ok: true, data: cached }

  const range = selection.range
  const now = new Date()
  // Only the `all` answer needs trimming; the worker windows the others itself.
  const since = downloadRangeMap[range] === 'all' ? sinceDay(range, now) : ''
  const updateSince = updateActivitySince(range, now)
  try {
    const [downloadsRaw, updateActivityRaw] = await Promise.all([
      fetchWorkerEndpoint<WorkerDownloadRow[]>(
        env.LICENSE_SERVER_ADMIN_TOKEN,
        `/admin/downloads?range=${downloadRangeMap[range]}`,
        env.WORKER_BASE_URL,
      ),
      fetchWorkerEndpoint<WorkerUpdateActivityRow[]>(
        env.LICENSE_SERVER_ADMIN_TOKEN,
        `/admin/update-activity?range=${updateRangeMap[range]}`,
        env.WORKER_BASE_URL,
      ),
    ])

    const data: CloudflareData = {
      downloads: parseDownloadRows(downloadsRaw).filter((r) => r.day >= since),
      updateActivity: parseUpdateActivityRows(updateActivityRaw).filter((r) => r.day >= updateSince),
    }
    await cacheSet(cacheSource, selectionCacheKey(selection), data)
    return { ok: true, data }
  } catch (e) {
    return { ok: false, error: `Cloudflare: ${e instanceof Error ? e.message : String(e)}` }
  }
}
