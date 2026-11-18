import type { SourceResult } from '../types.js'
import { cacheGet, cacheSet } from '../cache.js'
import { fetchWorkerEndpoint, WorkerHttpError } from './worker-endpoint.js'

/** Mirrors `ActiveInstallsDay` in `apps/api-server/src/admin/active-installs.ts`. */
export interface ActiveInstallsDay {
  /** UTC `YYYY-MM-DD`. */
  date: string
  /** Distinct installs that beat this day. */
  dau: number
  /** Installs whose first-ever beat fell on this day. */
  newInstalls: number
  /** The app's runtime this day, summed over installs, in hours. */
  appHours: number
  /** Distinct installs with any beat in [day − 6, day]. */
  wau: number
  /** Distinct installs with any beat in [day − 27, day]. */
  mau: number
  /** Installs by the highest version they ran this day; sums to `dau`. */
  byVersion: Record<string, number>
  /** Installs by OS (`macos`, `windows`, `linux`); sums to `dau`. Absent from a Worker older than the field. */
  byOs?: Record<string, number>
}

/** Mirrors `VersionWindow` in `apps/api-server/src/admin/active-installs.ts`. */
export interface VersionWindow {
  days: 7 | 30 | 90
  /** First day, inclusive. */
  since: string
  /** Last day (yesterday), inclusive: complete days only. */
  until: string
  /** Distinct installs with any beat in the window. */
  installs: number
  /**
   * Distinct installs with any beat on each version in the window ("installs exposed to v"). An install that
   * upgraded counts under both, so the values can sum past `installs`.
   */
  byVersion: Record<string, number>
}

/** Mirrors `ActiveInstallsResponse`: the last 181 UTC days (oldest first, today partial) plus the windows. */
export interface ActiveInstallsData {
  days: ActiveInstallsDay[]
  windows: VersionWindow[]
}

interface ActiveInstallsEnv {
  LICENSE_SERVER_ADMIN_TOKEN: string
  /** Optional override for the api-server base URL (local QA). Defaults to production. */
  WORKER_BASE_URL?: string
}

/** Half a year of complete days plus today: the 90-day stats and the 90 days they compare against. */
const fetchDays = 181

/** Each fetch reads every heartbeat row since ~208 days back, and the numbers move by the day, so an hour. */
const cacheTtlSeconds = 3600

const cacheSource = 'active-installs-v1'

/**
 * The heartbeat population per day and per version window, from `/admin/active-installs`. `data: null`
 * means the Worker doesn't have the endpoint yet (the dashboard deploys beside it, so it can briefly run
 * against the old one): the page reads that as "Not available yet". Only a real answer is cached, so the
 * null never outlives the Worker deploy.
 */
export async function fetchActiveInstallsData(
  env: ActiveInstallsEnv,
): Promise<SourceResult<ActiveInstallsData | null>> {
  const key = `days:${String(fetchDays)}`
  const cached = await cacheGet<ActiveInstallsData>(cacheSource, key)
  if (cached) return { ok: true, data: cached }

  try {
    const data = await fetchWorkerEndpoint<ActiveInstallsData>(
      env.LICENSE_SERVER_ADMIN_TOKEN,
      `/admin/active-installs?days=${String(fetchDays)}`,
      env.WORKER_BASE_URL,
    )
    await cacheSet(cacheSource, key, data, undefined, cacheTtlSeconds)
    return { ok: true, data }
  } catch (e) {
    if (e instanceof WorkerHttpError && e.status === 404) return { ok: true, data: null }
    return { ok: false, error: `Active installs: ${e instanceof Error ? e.message : String(e)}` }
  }
}
