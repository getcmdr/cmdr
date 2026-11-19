import { describe, it, expect } from 'vitest'
import type { ActiveInstallsDay } from './server/sources/active-installs.js'
import type { FunnelRow } from './server/sources/funnel.js'
import type { PaddleTransaction } from './money.js'
import { addDays } from './acquisition-stats.js'
import type { UpdateActivityRow } from './server/sources/cloudflare.js'
import {
  analyticsOnTile,
  d7Tile,
  hoursPerActiveDayTile,
  mauTile,
  payingShareTile,
  stickinessTile,
  wauTile,
} from './product-stats.js'

const today = '2026-08-10'

/** `count` days ending today (partial), built by `make(i)` where i = 0 is the oldest. */
function series(count: number, make: (i: number) => Partial<ActiveInstallsDay>): ActiveInstallsDay[] {
  return Array.from({ length: count }, (_, i) => ({
    date: addDays(today, i - count + 1),
    dau: 10,
    newInstalls: 1,
    appHours: 20,
    wau: 40,
    mau: 100,
    byVersion: {},
    ...make(i),
  }))
}

describe('wauTile and mauTile', () => {
  it("read yesterday's value and compare with the value N days earlier", () => {
    // 15 days: index 14 is today, 13 yesterday, 6 is yesterday − 7.
    const days = series(15, (i) => ({
      wau: i === 13 ? 60 : i === 6 ? 40 : i === 14 ? 999 : 50,
      mau: i === 13 ? 90 : 100,
    }))
    expect(wauTile(days, 7, today)).toEqual({ value: 60, change: 0.5 })
    const mau = mauTile(days, 7, today)
    expect(mau.value).toBe(90)
    expect(mau.change).toBeCloseTo(-0.1)
  })

  it('has no change when the earlier day is missing or zero', () => {
    expect(
      wauTile(
        series(3, () => ({})),
        7,
        today,
      ),
    ).toEqual({ value: 40, change: null })
    const zero = series(15, (i) => ({ wau: i === 6 ? 0 : 40 }))
    expect(wauTile(zero, 7, today).change).toBeNull()
  })

  it('is unknown without yesterday', () => {
    expect(wauTile([], 7, today)).toEqual({ value: null, change: null })
  })
})

describe('stickinessTile', () => {
  it('pools Σ DAU ÷ Σ MAU over complete days, today left out', () => {
    // Previous 7 days: 10/100 each. Last 7 complete days: 20/100 each. Today: 100/100, ignored.
    const days = series(15, (i) => ({ dau: i === 14 ? 100 : i >= 7 ? 20 : 10 }))
    const tile = stickinessTile(days, 7, today)
    expect(tile.value).toBeCloseTo(0.2)
    expect(tile.change).toBeCloseTo(0.1)
  })

  it('has no change when the previous window is not fully covered', () => {
    const tile = stickinessTile(
      series(10, () => ({})),
      7,
      today,
    )
    expect(tile.value).toBeCloseTo(0.1)
    expect(tile.change).toBeNull()
  })
})

describe('hoursPerActiveDayTile', () => {
  it('pools Σ app hours ÷ Σ DAU and compares relatively', () => {
    const days = series(15, (i) => ({ appHours: i >= 7 ? 30 : 20 }))
    const tile = hoursPerActiveDayTile(days, 7, today)
    expect(tile.value).toBeCloseTo(3)
    expect(tile.change).toBeCloseTo(0.5)
  })
})

describe('d7Tile', () => {
  function row(date: string, newInstalls: number, d7Retained: number | null): FunnelRow {
    const d7Retention = d7Retained === null ? null : d7Retained / newInstalls
    return { date, newInstalls, d7Retained, d7Retention, retentionSoFar: null } as unknown as FunnelRow
  }

  it('pools the final cohorts and compares with the cohorts before them', () => {
    // Final cohorts end at today − 8 (2026-08-02). Last 2: 3 of 10; the 2 before: 1 of 10.
    const rows = Array.from({ length: 12 }, (_, i) => {
      const date = addDays(today, i - 11)
      const final = date <= '2026-08-02'
      return row(date, 5, final ? (date >= '2026-08-01' ? 1.5 : 0.5) : null)
    })
    const tile = d7Tile(rows, 2, today)
    expect(tile.value).toBeCloseTo(0.3)
    expect(tile.change).toBeCloseTo(0.2)
  })

  it('is unknown without the funnel', () => {
    expect(d7Tile(null, 7, today)).toEqual({ value: null, change: null })
  })
})

describe('payingShareTile', () => {
  const txn = (createdAt: string, origin = 'web'): PaddleTransaction => ({
    id: createdAt,
    status: 'completed',
    createdAt,
    total: '1000',
    currencyCode: 'USD',
    origin,
    payout: null,
  })

  it('divides paid licenses through yesterday by MAU, against N days earlier', () => {
    const days = series(15, (i) => ({ mau: i === 13 ? 50 : 40 }))
    const txns = [
      txn('2026-07-01T10:00:00Z'),
      txn('2026-08-05T10:00:00Z'),
      txn('2026-08-06T10:00:00Z', 'subscription_recurring'),
      txn('2026-08-10T01:00:00Z'), // today: not yet
    ]
    const tile = payingShareTile(txns, days, 7, today)
    expect(tile.value).toBeCloseTo(2 / 50)
    expect(tile.change).toBeCloseTo(2 / 50 - 1 / 40)
  })

  it('is unknown without MAU', () => {
    expect(payingShareTile([], [], 7, today)).toEqual({ value: null, change: null })
  })
})

describe('analyticsOnTile', () => {
  /** One update-check row per day of `days`, split over two versions so the tile has to sum them. */
  function reach(days: ActiveInstallsDay[], count: (i: number) => number | null): UpdateActivityRow[] {
    return days.flatMap((d, i) => {
      const c = count(i)
      if (c === null) return []
      return [
        { day: d.date, version: '0.1.0', updaters: c / 2 },
        { day: d.date, version: '0.2.0', updaters: c / 2 },
      ]
    })
  }

  it('pools Σ DAU ÷ Σ update-checking addresses over complete days, today left out, change in points', () => {
    // Previous 7 days: 10 of 20 each. Last 7 complete: 10 of 40 on one day, 10 of 16 on the rest. Today: ignored.
    const days = series(15, () => ({}))
    const updates = reach(days, (i) => (i === 14 ? 10 : i === 13 ? 40 : i >= 7 ? 16 : 20))
    const tile = analyticsOnTile(days, updates, 7, today)
    // A ratio of sums, not a mean of ratios: 70 ÷ (40 + 6 × 16).
    expect(tile.value).toBeCloseTo(70 / 136)
    expect(tile.change).toBeCloseTo(70 / 136 - 0.5)
  })

  it('is unknown when a day in the window has no update-check data', () => {
    const days = series(15, () => ({}))
    const updates = reach(days, (i) => (i === 10 ? null : 20))
    expect(analyticsOnTile(days, updates, 7, today)).toEqual({ value: null, change: null })
  })

  it('has no change when the update-check data only covers the current window', () => {
    const days = series(15, () => ({}))
    const updates = reach(days, (i) => (i >= 7 ? 20 : null))
    const tile = analyticsOnTile(days, updates, 7, today)
    expect(tile.value).toBeCloseTo(0.5)
    expect(tile.change).toBeNull()
  })

  it('is unknown without any update checks', () => {
    expect(
      analyticsOnTile(
        series(15, () => ({})),
        [],
        7,
        today,
      ),
    ).toEqual({ value: null, change: null })
  })
})
