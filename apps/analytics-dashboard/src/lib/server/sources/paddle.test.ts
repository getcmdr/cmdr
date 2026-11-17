import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  fetchPaddleData,
  fetchPaddlePurchasesByDay,
  parseTransaction,
  parseSubscription,
  countSubscriptionsByStatus,
} from './paddle.js'
import { clearMemoryCache } from '../cache.js'

const mockEnv = { PADDLE_API_KEY_LIVE: 'test-paddle-key' }

const sampleRawTransaction = {
  id: 'txn_01abc123',
  status: 'completed',
  created_at: '2026-03-15T10:30:00Z',
  origin: 'web',
  details: {
    totals: { total: '5900', currency_code: 'USD' },
    payout_totals: { total: '5900', earnings: '5100', currency_code: 'USD' },
  },
}

const sampleRawSubscription = {
  id: 'sub_01xyz789',
  status: 'active',
  customer_id: 'ctm_01def456',
  current_billing_period: {
    starts_at: '2026-03-01T00:00:00Z',
    ends_at: '2026-04-01T00:00:00Z',
  },
}

function page<T>(data: T[], next?: string) {
  return {
    ok: true,
    json: () =>
      Promise.resolve({ data, meta: { pagination: { has_more: next !== undefined, next, estimated_total: 0 } } }),
  }
}

describe('parseTransaction', () => {
  it('parses a complete transaction', () => {
    expect(parseTransaction(sampleRawTransaction)).toEqual({
      id: 'txn_01abc123',
      status: 'completed',
      createdAt: '2026-03-15T10:30:00Z',
      total: '5900',
      currencyCode: 'USD',
      origin: 'web',
      payout: { total: '5900', earnings: '5100', currencyCode: 'USD' },
    })
  })

  it('handles missing totals, origin, and payout', () => {
    const result = parseTransaction({ id: 'txn_02', status: 'completed', created_at: '2026-03-15T10:30:00Z' })
    expect(result.total).toBe('0')
    expect(result.currencyCode).toBe('USD')
    expect(result.origin).toBeNull()
    expect(result.payout).toBeNull()
  })
})

describe('parseSubscription', () => {
  it('parses a subscription with billing period', () => {
    expect(parseSubscription(sampleRawSubscription)).toEqual({
      id: 'sub_01xyz789',
      status: 'active',
      customerId: 'ctm_01def456',
      currentBillingPeriod: { startsAt: '2026-03-01T00:00:00Z', endsAt: '2026-04-01T00:00:00Z' },
    })
  })

  it('handles null and missing billing periods', () => {
    expect(
      parseSubscription({ id: 'sub_02', status: 'canceled', customer_id: 'ctm_02', current_billing_period: null })
        .currentBillingPeriod,
    ).toBeNull()
    expect(
      parseSubscription({ id: 'sub_03', status: 'past_due', customer_id: 'ctm_03' }).currentBillingPeriod,
    ).toBeNull()
  })
})

describe('countSubscriptionsByStatus', () => {
  it('counts subscriptions by status', () => {
    const subs = [
      { id: '1', status: 'active', customerId: 'c1', currentBillingPeriod: null },
      { id: '2', status: 'active', customerId: 'c2', currentBillingPeriod: null },
      { id: '3', status: 'canceled', customerId: 'c3', currentBillingPeriod: null },
    ]
    expect(countSubscriptionsByStatus(subs)).toEqual({ active: 2, canceled: 1 })
    expect(countSubscriptionsByStatus([])).toEqual({})
  })
})

describe('fetchPaddleData', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
    clearMemoryCache()
  })

  it('fetches every completed transaction once, with no date filter', async () => {
    const fetchMock = vi
      .fn()
      .mockImplementation((url: string) =>
        Promise.resolve(url.includes('/transactions') ? page([sampleRawTransaction]) : page([sampleRawSubscription])),
      )
    vi.stubGlobal('fetch', fetchMock)

    const result = await fetchPaddleData(mockEnv, { range: '7d', end: null })
    expect(result.ok).toBe(true)
    if (!result.ok) return

    const transactionUrls = fetchMock.mock.calls
      .map((call: unknown[]) => String(call[0]))
      .filter((u) => u.includes('/transactions'))
    expect(transactionUrls).toEqual(['https://api.paddle.com/transactions?status=completed&per_page=200'])
    expect(result.data.allTransactions).toHaveLength(1)
    expect(result.data.activeSubscriptions).toHaveLength(1)
    expect(result.data.subscriptionsByStatus).toEqual({ active: 1 })
  })

  it('windows the legacy `transactions` list to the range in TypeScript', async () => {
    const recent = {
      ...sampleRawTransaction,
      id: 'recent',
      created_at: new Date(Date.now() - 2 * 86_400_000).toISOString(),
    }
    const old = { ...sampleRawTransaction, id: 'old', created_at: new Date(Date.now() - 20 * 86_400_000).toISOString() }
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockImplementation((url: string) =>
          Promise.resolve(url.includes('/transactions') ? page([recent, old]) : page([])),
        ),
    )

    const week = await fetchPaddleData(mockEnv, { range: '7d', end: null })
    const month = await fetchPaddleData(mockEnv, { range: '30d', end: null })
    if (!week.ok || !month.ok) throw new Error('expected data')
    expect(week.data.transactions.map((t) => t.id)).toEqual(['recent'])
    expect(month.data.transactions.map((t) => t.id)).toEqual(['recent', 'old'])
    expect(week.data.allTransactions).toHaveLength(2)
  })

  it('follows `next` as the full URL Paddle gives', async () => {
    const next = 'https://api.paddle.com/transactions?status=completed&per_page=200&after=txn_01abc123'
    const fetchMock = vi.fn().mockImplementation((url: string) => {
      if (url === next) return Promise.resolve(page([{ ...sampleRawTransaction, id: 'txn_02' }]))
      if (url.includes('/transactions')) return Promise.resolve(page([sampleRawTransaction], next))
      return Promise.resolve(page([]))
    })
    vi.stubGlobal('fetch', fetchMock)

    const result = await fetchPaddleData(mockEnv, { range: '30d', end: null })
    if (!result.ok) throw new Error(result.error)
    expect(result.data.allTransactions.map((t) => t.id)).toEqual(['txn_01abc123', 'txn_02'])
  })

  it('refuses a `next` that points off Paddle, so the API key never goes elsewhere', async () => {
    const fetchMock = vi
      .fn()
      .mockImplementation((url: string) =>
        Promise.resolve(
          url.includes('/transactions') ? page([sampleRawTransaction], 'https://evil.example.com/steal') : page([]),
        ),
      )
    vi.stubGlobal('fetch', fetchMock)

    const result = await fetchPaddleData(mockEnv, { range: '30d', end: null })
    expect(result.ok).toBe(false)
    expect(fetchMock.mock.calls.some((call: unknown[]) => String(call[0]).includes('evil'))).toBe(false)
  })

  it('returns an error when the API fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 401 }))
    const result = await fetchPaddleData(mockEnv, { range: '7d', end: null })
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.error).toContain('Paddle')
    expect(result.error).toContain('401')
  })
})

describe('fetchPaddlePurchasesByDay', () => {
  beforeEach(() => {
    vi.restoreAllMocks()
  })

  it("filters with Paddle's uppercase operator and counts first purchases only", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(
        page([
          sampleRawTransaction,
          { ...sampleRawTransaction, id: 'renewal', origin: 'subscription_recurring' },
          { ...sampleRawTransaction, id: 'card', origin: 'subscription_payment_method_change' },
          { ...sampleRawTransaction, id: 'plan', origin: 'subscription_update' },
          { ...sampleRawTransaction, id: 'api', origin: 'api', created_at: '2026-03-16T01:00:00Z' },
        ]),
      )
    vi.stubGlobal('fetch', fetchMock)

    const byDay = await fetchPaddlePurchasesByDay(mockEnv, '2026-03-01')
    expect(String(fetchMock.mock.calls[0][0])).toContain('created_at[GTE]=2026-03-01T00:00:00.000Z')
    expect(Object.fromEntries(byDay)).toEqual({ '2026-03-15': 1, '2026-03-16': 1 })
  })
})
