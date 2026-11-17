import type { DashboardSelection, SourceResult } from '../types.js'
import { rangeDays } from '../types.js'
import { cacheGet, cacheSet } from '../cache.js'
import { fetchWorkerEndpoint } from './worker-endpoint.js'
import type { FeatureUsageData, FeatureUsagePeriods } from '../../feature-usage.js'
import { shiftDay } from '../../time-range.js'

interface FeatureUsageEnv {
  LICENSE_SERVER_ADMIN_TOKEN: string
  /** Optional override for the api-server base URL (local QA). Defaults to production. */
  WORKER_BASE_URL?: string
}

/** The bounded windows' cache, keyed by range and last day (shape: `FeatureUsageData` with `until`). */
const boundedCacheSource = 'feature-usage-v2'

/**
 * One window of complete days ending `end` (inclusive). Cached only when the Worker honored `end` (the
 * answer carries `until`): an older Worker ignores it and answers its rolling window, which must not
 * outlive the Worker deploy in the cache.
 */
async function fetchWindow(env: FeatureUsageEnv, range: string, end: string): Promise<FeatureUsageData> {
  const extra = `end=${end}`
  const cached = await cacheGet<FeatureUsageData>(boundedCacheSource, range, extra)
  if (cached) return cached
  const data = await fetchWorkerEndpoint<FeatureUsageData>(
    env.LICENSE_SERVER_ADMIN_TOKEN,
    `/admin/feature-usage?range=${range}&${extra}`,
    env.WORKER_BASE_URL,
  )
  if (data.until !== undefined) await cacheSet(boundedCacheSource, range, data, extra)
  return data
}

/**
 * The Product page's feature usage: the range's complete days ending yesterday, and the same length before
 * it for the change column. On a Worker without `?end=` (no `until` in the answer), `previous` is null and
 * `current` is that Worker's rolling window, today included.
 */
export async function fetchFeatureUsagePeriods(
  env: FeatureUsageEnv,
  selection: DashboardSelection,
  today: string = new Date().toISOString().slice(0, 10),
): Promise<SourceResult<FeatureUsagePeriods>> {
  const yesterday = shiftDay(today, -1)
  try {
    const [current, previous] = await Promise.all([
      fetchWindow(env, selection.range, yesterday),
      fetchWindow(env, selection.range, shiftDay(yesterday, -rangeDays[selection.range])),
    ])
    return { ok: true, data: { current, previous: current.until === undefined ? null : previous } }
  } catch (e) {
    return { ok: false, error: `Feature usage: ${e instanceof Error ? e.message : String(e)}` }
  }
}
