import { afterEach, describe, expect, it, vi } from 'vitest'
import { app } from '../index'

/** `/download/:version/:os/:arch`, the OS-aware route, and its public-OS gate. The macOS-only route: `download-and-update-check.test.ts`. */

const macUa = { 'user-agent': 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15' }
const windowsUa = { 'user-agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/131.0' }

function recordingDb() {
  const bindMock = vi.fn((...args: unknown[]) => ({ args, run: () => Promise.resolve({ success: true }) }))
  const prepareMock = vi.fn(() => ({ bind: bindMock }))
  return { db: { prepare: prepareMock } as unknown as D1Database, prepareMock, bindMock }
}

function bindings(overrides: Record<string, unknown> = {}) {
  return { TELEMETRY_DB: recordingDb().db, PRODUCT_NAME: 'Cmdr', SUPPORT_EMAIL: 'test@example.com', ...overrides }
}

const withWindows = { PUBLIC_DOWNLOAD_OSES: 'macos,windows' }

/** `latest.json` as a release with Windows rows writes it (`scripts/release-assets/latest_test.go`). */
function manifestWithWindows(version: string) {
  return {
    version,
    downloads: [
      { os: 'macos', arch: 'universal', kind: 'dmg', file: `Cmdr_${version}_universal.dmg`, size: 1 },
      { os: 'windows', arch: 'x86_64', kind: 'nsis', file: `Cmdr_${version}_x64-setup.exe`, size: 2 },
      { os: 'windows', arch: 'x86_64', kind: 'msix', file: `Cmdr_${version}_x64.msix`, size: 3 },
    ],
  }
}

const releaseManifestUrl = (version: string) =>
  `https://github.com/getcmdr/cmdr/releases/download/v${version}/latest.json`

/** Answers each manifest URL in `manifests` with its body, anything else with a 404. */
function stubManifests(manifests: Record<string, unknown>) {
  const fetchMock = vi.fn((input: RequestInfo | URL) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
    const body = manifests[url]
    if (body === undefined) return Promise.resolve(new Response(null, { status: 404 }))
    return Promise.resolve(new Response(JSON.stringify(body), { status: 200 }))
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('GET /download/:version/macos/:arch', () => {
  it('serves a DMG the same way the old route does, without a manifest lookup', async () => {
    const fetchMock = stubManifests({})
    const res = await app.request('/download/0.61.0/macos/x86_64', { headers: macUa }, bindings())

    expect(res.status).toBe(302)
    expect(res.headers.get('location')).toBe('https://download.getcmdr.com/v0.61.0/Cmdr_0.61.0_x64.dmg')
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('records the OS and kind beside the rest of the row', async () => {
    const { db, bindMock } = recordingDb()
    await app.request('/download/0.61.0/macos/aarch64', { headers: macUa }, bindings({ TELEMETRY_DB: db }))

    // [app_version, arch, country, continent, hashed_ip, source, ref, referer, user_agent, ua_family, os, kind]
    expect(bindMock.mock.calls[0].slice(9)).toEqual(['human', 'macos', 'dmg'])
  })

  it('records the old route as a macOS DMG too', async () => {
    const { db, bindMock } = recordingDb()
    await app.request('/download/0.61.0/aarch64', { headers: macUa }, bindings({ TELEMETRY_DB: db }))

    expect(bindMock.mock.calls[0].slice(10)).toEqual(['macos', 'dmg'])
  })

  it('serves the pkg by ?kind=, from the release manifest', async () => {
    stubManifests({
      [releaseManifestUrl('0.70.0')]: {
        downloads: [{ os: 'macos', arch: 'universal', kind: 'pkg', file: 'Cmdr_0.70.0_universal.pkg', size: 1 }],
      },
    })
    const res = await app.request('/download/0.70.0/macos/universal?kind=pkg', { headers: macUa }, bindings())

    expect(res.headers.get('location')).toBe('https://download.getcmdr.com/v0.70.0/Cmdr_0.70.0_universal.pkg')
  })
})

describe('GET /download/:version/windows/:arch while Windows is not public', () => {
  it('answers 404 and never looks the build up or logs it', async () => {
    const fetchMock = stubManifests({ [releaseManifestUrl('0.70.0')]: manifestWithWindows('0.70.0') })
    const { db, prepareMock } = recordingDb()

    const res = await app.request(
      '/download/0.70.0/windows/x86_64',
      { headers: windowsUa },
      bindings({ TELEMETRY_DB: db, PUBLIC_DOWNLOAD_OSES: 'macos' }),
    )

    expect(res.status).toBe(404)
    expect(fetchMock).not.toHaveBeenCalled()
    expect(prepareMock).not.toHaveBeenCalled()
  })

  it('reads an absent var as macOS only, so a Worker without it exposes nothing new', async () => {
    stubManifests({ [releaseManifestUrl('0.70.0')]: manifestWithWindows('0.70.0') })
    const res = await app.request('/download/0.70.0/windows/x86_64', { headers: windowsUa }, bindings())

    expect(res.status).toBe(404)
  })

  it.each(['', ' ', 'windows', 'linux,windows', 'macOS'])(
    'keeps serving the macOS DMG on both routes when the var reads %j: it was public before the gate',
    async (value) => {
      stubManifests({})
      const env = bindings({ PUBLIC_DOWNLOAD_OSES: value })
      for (const path of ['/download/0.61.0/aarch64', '/download/0.61.0/macos/aarch64']) {
        const res = await app.request(path, { headers: macUa }, env)
        expect(res.status, path).toBe(302)
        expect(res.headers.get('location')).toBe('https://download.getcmdr.com/v0.61.0/Cmdr_0.61.0_aarch64.dmg')
      }
    },
  )

  it('answers 404 for latest too, before resolving it', async () => {
    const fetchMock = stubManifests({ 'https://getcmdr.com/latest.json': manifestWithWindows('0.70.0') })
    const res = await app.request('/download/latest/windows/x86_64', { headers: windowsUa }, bindings())

    expect(res.status).toBe(404)
    expect(fetchMock).not.toHaveBeenCalled()
  })
})

describe('GET /download/:version/windows/:arch once Windows is public', () => {
  it("redirects to the NSIS installer the release's own manifest names", async () => {
    stubManifests({ [releaseManifestUrl('0.70.0')]: manifestWithWindows('0.70.0') })
    const res = await app.request('/download/0.70.0/windows/x86_64', { headers: windowsUa }, bindings(withWindows))

    expect(res.status).toBe(302)
    expect(res.headers.get('location')).toBe('https://download.getcmdr.com/v0.70.0/Cmdr_0.70.0_x64-setup.exe')
  })

  it('serves another kind by ?kind=', async () => {
    stubManifests({ [releaseManifestUrl('0.70.0')]: manifestWithWindows('0.70.0') })
    const res = await app.request(
      '/download/0.70.0/windows/x86_64?kind=msix',
      { headers: windowsUa },
      bindings(withWindows),
    )

    expect(res.headers.get('location')).toBe('https://download.getcmdr.com/v0.70.0/Cmdr_0.70.0_x64.msix')
  })

  it("resolves latest through getcmdr.com/latest.json and reads that manifest's downloads, in one fetch", async () => {
    const fetchMock = stubManifests({ 'https://getcmdr.com/latest.json': manifestWithWindows('0.70.0') })
    const { db, bindMock } = recordingDb()

    const res = await app.request(
      '/download/latest/windows/x86_64',
      { headers: windowsUa },
      bindings({ ...withWindows, TELEMETRY_DB: db }),
    )

    expect(res.headers.get('location')).toBe('https://download.getcmdr.com/v0.70.0/Cmdr_0.70.0_x64-setup.exe')
    expect(fetchMock).toHaveBeenCalledOnce()
    expect(bindMock.mock.calls[0][0]).toBe('0.70.0')
  })

  it('classifies a Windows browser fetching the installer as a possible install', async () => {
    stubManifests({ [releaseManifestUrl('0.70.0')]: manifestWithWindows('0.70.0') })
    const { db, bindMock } = recordingDb()

    await app.request(
      '/download/0.70.0/windows/x86_64',
      { headers: windowsUa },
      bindings({ ...withWindows, TELEMETRY_DB: db }),
    )

    expect(bindMock.mock.calls[0].slice(9)).toEqual(['human', 'windows', 'nsis'])
  })

  it('classifies a Mac fetching the installer as bot', async () => {
    stubManifests({ [releaseManifestUrl('0.70.0')]: manifestWithWindows('0.70.0') })
    const { db, bindMock } = recordingDb()

    await app.request(
      '/download/0.70.0/windows/x86_64',
      { headers: macUa },
      bindings({ ...withWindows, TELEMETRY_DB: db }),
    )

    expect(bindMock.mock.calls[0][9]).toBe('bot')
  })

  it('answers 404 and logs nothing for a release without that build', async () => {
    stubManifests({ [releaseManifestUrl('0.61.0')]: { version: '0.61.0', downloads: [] } })
    const { db, prepareMock } = recordingDb()

    const res = await app.request(
      '/download/0.61.0/windows/x86_64',
      { headers: windowsUa },
      bindings({ ...withWindows, TELEMETRY_DB: db }),
    )

    expect(res.status).toBe(404)
    expect(prepareMock).not.toHaveBeenCalled()
  })

  it('answers 404 when the release manifest is unreachable', async () => {
    stubManifests({})
    const res = await app.request('/download/0.70.0/windows/x86_64', { headers: windowsUa }, bindings(withWindows))

    expect(res.status).toBe(404)
  })

  it('refuses a manifest filename that could leave the release folder', async () => {
    stubManifests({
      [releaseManifestUrl('0.70.0')]: {
        downloads: [{ os: 'windows', arch: 'x86_64', kind: 'nsis', file: '../evil.exe', size: 1 }],
      },
    })
    const res = await app.request('/download/0.70.0/windows/x86_64', { headers: windowsUa }, bindings(withWindows))

    expect(res.status).toBe(404)
  })
})

describe('GET /download/:version/:os/:arch input checks', () => {
  it.each([
    ['an unknown OS', '/download/0.70.0/freebsd/x86_64'],
    ['an arch the OS has no build for', '/download/0.70.0/windows/universal'],
    ['a kind of another OS', '/download/0.70.0/windows/x86_64?kind=dmg'],
    ['a malformed version', '/download/0.70/macos/x86_64'],
  ])('answers 400 for %s', async (_label, path) => {
    const res = await app.request(path, { headers: windowsUa }, bindings(withWindows))

    expect(res.status).toBe(400)
  })
})
