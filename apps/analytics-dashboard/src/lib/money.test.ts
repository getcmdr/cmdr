import { describe, it, expect } from 'vitest'
import {
  allTimeRevenue,
  isFirstPurchase,
  paidLicensesThrough,
  recentPurchases,
  revenueInPeriod,
  sumByCurrency,
  type PaddleTransaction,
} from './money.js'

function txn(overrides: Partial<PaddleTransaction> & { createdAt: string }): PaddleTransaction {
  return {
    id: `txn_${overrides.createdAt}`,
    status: 'completed',
    total: '1000',
    currencyCode: 'USD',
    origin: 'web',
    payout: null,
    ...overrides,
  }
}

const today = '2026-08-10'

describe('isFirstPurchase', () => {
  it('counts the checkout and API-created purchases only', () => {
    expect(isFirstPurchase({ origin: 'web' })).toBe(true)
    expect(isFirstPurchase({ origin: 'api' })).toBe(true)
    expect(isFirstPurchase({ origin: 'subscription_recurring' })).toBe(false)
    expect(isFirstPurchase({ origin: 'subscription_payment_method_change' })).toBe(false)
    expect(isFirstPurchase({ origin: 'subscription_update' })).toBe(false)
    expect(isFirstPurchase({ origin: 'subscription_charge' })).toBe(false)
    expect(isFirstPurchase({ origin: null })).toBe(false)
  })
})

describe('sumByCurrency', () => {
  it('never adds amounts across currencies', () => {
    const lines = sumByCurrency([
      txn({ createdAt: '2026-08-01T10:00:00Z', total: '3900', currencyCode: 'USD' }),
      txn({ createdAt: '2026-08-02T10:00:00Z', total: '3500', currencyCode: 'EUR' }),
      txn({ createdAt: '2026-08-03T10:00:00Z', total: '1000', currencyCode: 'USD' }),
    ])
    expect(lines).toEqual([
      { currencyCode: 'USD', amount: 4900 },
      { currencyCode: 'EUR', amount: 3500 },
    ])
  })

  it('prefers the payout amount, so mixed checkout currencies land in one payout currency', () => {
    const lines = sumByCurrency([
      txn({
        createdAt: '2026-08-01T10:00:00Z',
        total: '3500',
        currencyCode: 'EUR',
        payout: { total: '3800', earnings: '3300', currencyCode: 'USD' },
      }),
      txn({
        createdAt: '2026-08-02T10:00:00Z',
        total: '3900',
        currencyCode: 'USD',
        payout: { total: '3900', earnings: '3400', currencyCode: 'USD' },
      }),
    ])
    expect(lines).toEqual([{ currencyCode: 'USD', amount: 7700 }])
  })
})

describe('revenueInPeriod', () => {
  it('covers complete days only and compares with the window before', () => {
    const txns = [
      txn({ createdAt: '2026-08-10T01:00:00Z', total: '9999' }), // today: excluded
      txn({ createdAt: '2026-08-09T23:59:59Z', total: '2000' }), // yesterday: in
      txn({ createdAt: '2026-08-03T00:00:00Z', total: '1000' }), // first day of the 7: in
      txn({ createdAt: '2026-08-02T12:00:00Z', total: '1500' }), // previous window
    ]
    expect(revenueInPeriod(txns, 7, today)).toEqual([
      { currencyCode: 'USD', amount: 3000, previousAmount: 1500, change: 1 },
    ])
  })

  it('reads a change from a quiet previous window as unknown, never +∞', () => {
    const lines = revenueInPeriod([txn({ createdAt: '2026-08-09T10:00:00Z' })], 7, today)
    expect(lines[0].change).toBeNull()
    expect(lines[0].previousAmount).toBe(0)
  })

  it('keeps a currency that only sold in the previous window, at zero now', () => {
    const lines = revenueInPeriod([txn({ createdAt: '2026-08-01T10:00:00Z', currencyCode: 'EUR' })], 7, today)
    expect(lines).toEqual([{ currencyCode: 'EUR', amount: 0, previousAmount: 1000, change: -1 }])
  })
})

describe('allTimeRevenue', () => {
  it('sums everything through yesterday', () => {
    const txns = [txn({ createdAt: '2025-01-01T10:00:00Z' }), txn({ createdAt: '2026-08-10T01:00:00Z' })]
    expect(allTimeRevenue(txns, today)).toEqual([{ currencyCode: 'USD', amount: 1000 }])
  })
})

describe('paidLicensesThrough', () => {
  it('counts first purchases before the exclusive end day', () => {
    const txns = [
      txn({ createdAt: '2026-08-01T10:00:00Z' }),
      txn({ createdAt: '2026-08-02T10:00:00Z', origin: 'subscription_recurring' }),
      txn({ createdAt: '2026-08-03T10:00:00Z', origin: 'subscription_payment_method_change', total: '0' }),
      txn({ createdAt: '2026-08-09T10:00:00Z', origin: 'api' }),
      txn({ createdAt: '2026-08-10T01:00:00Z' }),
    ]
    expect(paidLicensesThrough(txns, today)).toBe(2)
    expect(paidLicensesThrough(txns, '2026-08-02')).toBe(1)
  })
})

describe('recentPurchases', () => {
  it("lists the window's first purchases newest first, today's marked", () => {
    const txns = [
      txn({ id: 'old', createdAt: '2026-08-02T10:00:00Z' }),
      txn({ id: 'renewal', createdAt: '2026-08-05T10:00:00Z', origin: 'subscription_recurring' }),
      txn({ id: 'mid', createdAt: '2026-08-05T10:00:00Z' }),
      txn({ id: 'now', createdAt: '2026-08-10T01:00:00Z' }),
    ]
    expect(recentPurchases(txns, 7, today)).toEqual([
      { id: 'now', createdAt: '2026-08-10T01:00:00Z', amount: { currencyCode: 'USD', amount: 1000 }, isToday: true },
      { id: 'mid', createdAt: '2026-08-05T10:00:00Z', amount: { currencyCode: 'USD', amount: 1000 }, isToday: false },
    ])
  })
})
