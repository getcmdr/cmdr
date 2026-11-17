import { describe, expect, it } from 'vitest'
import {
  foldNoData,
  formatFeatureShare,
  formatParts,
  noDataYet,
  shareBarWidth,
  shareChange,
  sortFeatureUsage,
  type FeatureUsage,
  type FeatureUsageData,
} from './feature-usage.js'

function feature(id: string, installs: number, events: number, everSeen = true): FeatureUsage {
  return { id, name: id, sources: [], installs, events, everSeen, parts: [] }
}

describe('formatFeatureShare', () => {
  it('reads an event no release has sent as "no data yet", never as 0%', () => {
    expect(formatFeatureShare(feature('multi-rename', 0, 0, false), 100)).toBe(noDataYet)
  })

  it('reads a shipped event nobody fired as a real 0%', () => {
    expect(formatFeatureShare(feature('tabs', 0, 0), 100)).toBe('0%')
  })

  it('shows a dash when there are no active installs to divide by', () => {
    expect(formatFeatureShare(feature('search', 0, 0), 0)).toBe('–')
  })

  it('shares installs over active installs', () => {
    expect(formatFeatureShare(feature('search', 25, 90), 100)).toBe('25%')
  })
})

describe('sortFeatureUsage', () => {
  it('puts the most-used first, keeps mapping order on ties, and never-seen features last', () => {
    const sorted = sortFeatureUsage([
      feature('never', 0, 0, false),
      feature('tie-a', 2, 5),
      feature('top', 9, 1),
      feature('tie-b', 2, 5),
      feature('zero', 0, 0),
    ])
    expect(sorted.map((f) => f.id)).toEqual(['top', 'tie-a', 'tie-b', 'zero', 'never'])
  })
})

describe('formatParts', () => {
  it('names each part with its events and installs', () => {
    expect(
      formatParts([
        { key: 'answered', installs: 3, events: 1_200 },
        { key: 'failed', installs: 1, events: 2 },
      ]),
    ).toBe('answered 1,200 (3 installs), failed 2 (1 install)')
  })
})

describe('shareChange', () => {
  const window = (since: string, activeInstalls: number, features: FeatureUsage[]): FeatureUsageData => ({
    range: '7d',
    since,
    until: '2026-08-10T00:00:00.000Z',
    activeInstalls,
    features,
  })
  const seen = (installs: number, firstSeenAt: string | null = '2026-01-01T00:00:00.000Z'): FeatureUsage => ({
    ...feature('search', installs, installs),
    firstSeenAt,
  })
  const previousSince = '2026-07-27T00:00:00.000Z'

  it('compares shares in percentage points', () => {
    const current = window('2026-08-03T00:00:00.000Z', 100, [seen(30)])
    const previous = window(previousSince, 50, [seen(10)])
    expect(shareChange(seen(30), { current, previous })).toBeCloseTo(0.1)
  })

  it("has no change when the feature's first event came after the previous window began", () => {
    const young = seen(30, '2026-07-30T12:00:00.000Z')
    const current = window('2026-08-03T00:00:00.000Z', 100, [young])
    const previous = window(previousSince, 50, [{ ...young, installs: 0 }])
    expect(shareChange(young, { current, previous })).toBeNull()
  })

  it('has no change without a previous window (an older Worker)', () => {
    expect(shareChange(seen(30), { current: window('2026-08-03T00:00:00.000Z', 100, []), previous: null })).toBeNull()
  })

  it('has no change for a feature that has never sent an event', () => {
    const never = { ...feature('tabs', 0, 0, false), firstSeenAt: null }
    const current = window('2026-08-03T00:00:00.000Z', 100, [never])
    expect(shareChange(never, { current, previous: window(previousSince, 50, [never]) })).toBeNull()
  })
})

describe('shareBarWidth', () => {
  it('is the share, and empty for no data yet', () => {
    expect(shareBarWidth(feature('search', 25, 90), 100)).toBe(0.25)
    expect(shareBarWidth(feature('tabs', 0, 0, false), 100)).toBe(0)
    expect(shareBarWidth(feature('search', 5, 5), 0)).toBe(0)
  })
})

describe('foldNoData', () => {
  it('splits features with stored events from the never-seen ones, in order', () => {
    const { seen, noData } = foldNoData([feature('a', 1, 1), feature('b', 0, 0, false), feature('c', 0, 0)])
    expect(seen.map((f) => f.id)).toEqual(['a', 'c'])
    expect(noData.map((f) => f.id)).toEqual(['b'])
  })
})
