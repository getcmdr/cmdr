import { describe, it, expect, vi, beforeEach } from 'vitest'
import { fetchActiveInstallsData, type ActiveInstallsData } from './active-installs.js'
import { clearMemoryCache } from '../cache.js'

const env = { LICENSE_SERVER_ADMIN_TOKEN: 'test-admin-token' }

const sample: ActiveInstallsData = {
  days: [
    {
      date: '2026-08-01',
      dau: 3,
      newInstalls: 1,
      appHours: 7.5,
      wau: 4,
      mau: 6,
      byVersion: { '0.61.0': 2, '0.60.0': 1 },
    },
  ],
  windows: [
    { days: 7, since: '2026-07-25', until: '2026-07-31', installs: 4, byVersion: { '0.61.0': 3, '0.60.0': 2 } },
  ],
}

function respond(status: number, body: unknown) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
    text: () => Promise.resolve(JSON.stringify(body)),
  }
}

describe('fetchActiveInstallsData', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    clearMemoryCache()
  })

  it('asks for 181 days and passes the answer through', async () => {
    const fetchMock = vi.fn().mockResolvedValue(respond(200, sample))
    vi.stubGlobal('fetch', fetchMock)

    const result = await fetchActiveInstallsData(env)
    expect(result).toEqual({ ok: true, data: sample })
    expect(fetchMock.mock.calls[0][0]).toBe('https://api.getcmdr.com/admin/active-installs?days=181')
  })

  it('reads a 404 (a Worker from before the endpoint) as null, not an error', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(respond(404, 'Not Found')))
    expect(await fetchActiveInstallsData(env)).toEqual({ ok: true, data: null })
  })

  it("doesn't cache the old Worker's null, so the new Worker shows up on the next load", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(respond(404, 'Not Found'))
      .mockResolvedValueOnce(respond(200, sample))
    vi.stubGlobal('fetch', fetchMock)

    expect(await fetchActiveInstallsData(env)).toEqual({ ok: true, data: null })
    expect(await fetchActiveInstallsData(env)).toEqual({ ok: true, data: sample })
  })

  it('caches a real answer', async () => {
    const fetchMock = vi.fn().mockResolvedValue(respond(200, sample))
    vi.stubGlobal('fetch', fetchMock)

    await fetchActiveInstallsData(env)
    await fetchActiveInstallsData(env)
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('reports any other failure as an error', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(respond(500, 'boom')))
    const result = await fetchActiveInstallsData(env)
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.error).toContain('Active installs')
  })
})
