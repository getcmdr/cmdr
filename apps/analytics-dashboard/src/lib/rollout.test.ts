import { describe, it, expect } from 'vitest'
import type { ActiveInstallsDay } from './server/sources/active-installs.js'
import type { GitHubRelease } from './server/sources/github.js'
import { compareVersions, latestRelease, rolloutHeadline, rolloutSentence, versionStack } from './rollout.js'

function day(date: string, byVersion: Record<string, number>): ActiveInstallsDay {
  const dau = Object.values(byVersion).reduce((a, b) => a + b, 0)
  return { date, dau, newInstalls: 0, appHours: 0, wau: dau, mau: dau, byVersion }
}

function release(tagName: string, publishedAt: string, flags: Partial<GitHubRelease> = {}): GitHubRelease {
  return { tagName, publishedAt, draft: false, prerelease: false, assets: [], totalDownloads: 0, ...flags }
}

const today = '2026-08-10'

describe('compareVersions', () => {
  it('compares numerically, never as strings', () => {
    expect(['0.10.0', '0.9.0', '0.9.1'].sort(compareVersions)).toEqual(['0.9.0', '0.9.1', '0.10.0'])
  })

  it('sorts a pre-release below its release without breaking the order', () => {
    expect(['0.62.0', '0.62.0-beta.1', '0.61.0'].sort(compareVersions)).toEqual(['0.61.0', '0.62.0-beta.1', '0.62.0'])
  })
})

describe('latestRelease', () => {
  it('picks the newest stable release, skipping drafts and prereleases, without the tag v', () => {
    const releases = [
      release('v0.62.0', '2026-08-09T10:00:00Z', { draft: true }),
      release('v0.62.0-beta.1', '2026-08-08T10:00:00Z', { prerelease: true }),
      release('v0.61.0', '2026-08-01T10:00:00Z'),
      release('v0.60.0', '2026-07-20T10:00:00Z'),
    ]
    expect(latestRelease(releases, [])).toEqual({ version: '0.61.0', publishedAt: '2026-08-01T10:00:00Z' })
  })

  it('falls back to the newest version seen, with no release date', () => {
    const days = [day('2026-08-08', { '0.9.0': 1 }), day('2026-08-09', { '0.10.0': 2, '0.9.0': 1 })]
    expect(latestRelease(null, days)).toEqual({ version: '0.10.0', publishedAt: null })
  })

  it('is null with nothing to go on', () => {
    expect(latestRelease([], [])).toBeNull()
  })
})

describe('rolloutHeadline', () => {
  const days = [day('2026-08-09', { '0.61.0': 3, '0.60.0': 1 }), day(today, { '0.61.0': 9 })]

  it("reads yesterday's share, days after the release", () => {
    expect(rolloutHeadline({ version: '0.61.0', publishedAt: '2026-08-01T10:00:00Z' }, days, today)).toEqual({
      kind: 'daysAfter',
      version: '0.61.0',
      daysAfter: 8,
      share: 0.75,
    })
  })

  it('says "on release day" for a release published yesterday', () => {
    expect(rolloutHeadline({ version: '0.61.0', publishedAt: '2026-08-09T10:00:00Z' }, days, today)).toEqual({
      kind: 'onReleaseDay',
      version: '0.61.0',
      share: 0.75,
    })
  })

  it('has no share for a release published today: no complete day exists on it yet', () => {
    expect(rolloutHeadline({ version: '0.62.0', publishedAt: '2026-08-10T08:00:00Z' }, days, today)).toEqual({
      kind: 'releasedToday',
      version: '0.62.0',
    })
  })

  it('reads a share without a day count for a fallback version', () => {
    expect(rolloutHeadline({ version: '0.61.0', publishedAt: null }, days, today)).toEqual({
      kind: 'noReleaseDate',
      version: '0.61.0',
      share: 0.75,
    })
  })

  it('reads an unknown share when yesterday is missing or had nobody', () => {
    const quiet = [day('2026-08-09', {})]
    expect(rolloutHeadline({ version: '0.61.0', publishedAt: null }, quiet, today)).toMatchObject({ share: null })
    expect(rolloutHeadline({ version: '0.61.0', publishedAt: null }, [], today)).toMatchObject({ share: null })
  })
})

describe('rolloutSentence', () => {
  it('names the share, the version, and the days since release', () => {
    expect(rolloutSentence({ kind: 'daysAfter', version: '0.61.0', daysAfter: 8, share: 0.75 })).toBe(
      '75% of active installs ran v0.61.0 yesterday, 8 days after its release',
    )
    expect(rolloutSentence({ kind: 'daysAfter', version: '0.61.0', daysAfter: 1, share: 0.4 })).toBe(
      '40% of active installs ran v0.61.0 yesterday, 1 day after its release',
    )
  })

  it('never prints a share or "−1 days" for a release published today', () => {
    expect(rolloutSentence({ kind: 'releasedToday', version: '0.62.0' })).toBe(
      'v0.62.0 came out today; its first full day is tomorrow',
    )
  })

  it('reads "on release day" for a release published yesterday', () => {
    expect(rolloutSentence({ kind: 'onReleaseDay', version: '0.62.0', share: 0.12 })).toBe(
      '12% of active installs ran v0.62.0 yesterday, its release day',
    )
  })

  it('drops the day count for a fallback version, and dashes an unknown share', () => {
    expect(rolloutSentence({ kind: 'noReleaseDate', version: '0.61.0', share: null })).toBe(
      '– of active installs ran v0.61.0 yesterday',
    )
  })
})

describe('versionStack', () => {
  it('stacks every version with an install, oldest first, zero-filled', () => {
    const stack = versionStack([day('2026-08-08', { '0.9.0': 2 }), day('2026-08-09', { '0.10.0': 3, '0.9.0': 1 })])
    expect(stack).toEqual({
      days: ['2026-08-08', '2026-08-09'],
      versions: ['0.9.0', '0.10.0'],
      series: { '0.9.0': [2, 1], '0.10.0': [0, 3] },
    })
  })

  it('leaves out a version with no installs in the view', () => {
    expect(versionStack([day('2026-08-08', { '0.9.0': 0, '0.10.0': 1 })]).versions).toEqual(['0.10.0'])
  })
})
