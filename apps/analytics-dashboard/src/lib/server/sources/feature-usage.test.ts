import { describe, it, expect, vi, beforeEach } from 'vitest'
import { fetchFeatureUsagePeriods } from './feature-usage.js'
import { clearMemoryCache } from '../cache.js'
import type { FeatureUsageData } from '../../feature-usage.js'

const env = { LICENSE_SERVER_ADMIN_TOKEN: 'test-admin-token' }
const selection = { range: '7d', end: null } as const
const today = '2026-08-10'

function answer(end: string | null): FeatureUsageData {
  return {
    range: '7d',
    since: end ? `${end}T00:00:00.000Z` : '2026-08-03T09:00:00.000Z',
    ...(end ? { until: `${end}T00:00:00.000Z` } : {}),
    activeInstalls: 10,
    features: [],
  }
}

function respond(body: unknown) {
  return { ok: true, status: 200, json: () => Promise.resolve(body), text: () => Promise.resolve('') }
}

describe('fetchFeatureUsagePeriods', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    clearMemoryCache()
  })

  it('asks for the window ending yesterday and the one before it', async () => {
    const fetchMock = vi.fn().mockImplementation((url: string) => Promise.resolve(respond(answer(url.slice(-10)))))
    vi.stubGlobal('fetch', fetchMock)

    const result = await fetchFeatureUsagePeriods(env, selection, today)
    if (!result.ok) throw new Error(result.error)
    expect(fetchMock.mock.calls.map((call: unknown[]) => String(call[0])).sort()).toEqual([
      'https://api.getcmdr.com/admin/feature-usage?range=7d&end=2026-08-02',
      'https://api.getcmdr.com/admin/feature-usage?range=7d&end=2026-08-09',
    ])
    expect(result.data.current.until).toBe('2026-08-09T00:00:00.000Z')
    expect(result.data.previous?.until).toBe('2026-08-02T00:00:00.000Z')
  })

  it("has no previous window on a Worker that ignores `end`, and doesn't cache its answer", async () => {
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(respond(answer(null))))
    vi.stubGlobal('fetch', fetchMock)

    const result = await fetchFeatureUsagePeriods(env, selection, today)
    if (!result.ok) throw new Error(result.error)
    expect(result.data.previous).toBeNull()
    expect(result.data.current.until).toBeUndefined()

    await fetchFeatureUsagePeriods(env, selection, today)
    expect(fetchMock).toHaveBeenCalledTimes(4)
  })

  it('caches a bounded answer', async () => {
    const fetchMock = vi.fn().mockImplementation((url: string) => Promise.resolve(respond(answer(url.slice(-10)))))
    vi.stubGlobal('fetch', fetchMock)

    await fetchFeatureUsagePeriods(env, selection, today)
    await fetchFeatureUsagePeriods(env, selection, today)
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })
})
