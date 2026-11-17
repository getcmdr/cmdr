import type { SourceResult } from '../types.js'
import { cacheGet, cacheSet } from '../cache.js'
import { isFirstPurchase, type PaddleTransaction } from '../../money.js'

export type { PaddleTransaction }

export interface PaddleData {
  /**
   * Every completed transaction, all time, oldest first as Paddle lists them. The money math windows this
   * itself (`#lib/money.ts`), so revenue, purchases, and paid licenses all read one fetch.
   */
  allTransactions: PaddleTransaction[]
}

interface PaddleEnv {
  PADDLE_API_KEY_LIVE: string
}

const paddleApiBase = 'https://api.paddle.com'

/** Paddle's largest page, so a full history takes as few round trips as possible. */
const transactionsPath = '/transactions?status=completed&per_page=200'

interface PaddleListResponse<T> {
  data: T[]
  /** `next` is the full URL of the next page (Paddle docs, pagination, checked 2026-11-17). */
  meta: { pagination: { next?: string; has_more: boolean; estimated_total: number } }
}

interface PaddleRawTotals {
  total?: string
  earnings?: string
  currency_code?: string
}

interface PaddleRawTransaction {
  id: string
  status: string
  created_at: string
  origin?: string | null
  details?: { totals?: PaddleRawTotals; payout_totals?: PaddleRawTotals | null }
}

async function paddleFetch<T>(apiKey: string, url: string): Promise<PaddleListResponse<T>> {
  const response = await fetch(url, {
    headers: { Authorization: `Bearer ${apiKey}` },
  })
  if (!response.ok) {
    throw new Error(`Paddle ${new URL(url).pathname} returned ${String(response.status)}`)
  }
  return (await response.json()) as PaddleListResponse<T>
}

/**
 * Paginates through all results by following Paddle's `next`, a full URL. It must point back at the
 * Paddle API: the request carries the API key, so a `next` anywhere else is refused, never fetched.
 */
async function paddleFetchAll<T>(apiKey: string, path: string): Promise<T[]> {
  const all: T[] = []
  let url: string | null = `${paddleApiBase}${path}`
  while (url) {
    const result: PaddleListResponse<T> = await paddleFetch<T>(apiKey, url)
    all.push(...result.data)
    const next = result.meta.pagination.has_more ? result.meta.pagination.next : undefined
    if (next !== undefined && !next.startsWith(`${paddleApiBase}/`)) {
      throw new Error(`Paddle pagination pointed off the API, refusing to follow it`)
    }
    url = next ?? null
  }
  return all
}

export function parseTransaction(raw: PaddleRawTransaction): PaddleTransaction {
  const payout = raw.details?.payout_totals
  return {
    id: raw.id,
    status: raw.status,
    createdAt: raw.created_at,
    total: raw.details?.totals?.total ?? '0',
    currencyCode: raw.details?.totals?.currency_code ?? 'USD',
    origin: raw.origin ?? null,
    payout:
      payout?.total !== undefined && payout.currency_code !== undefined
        ? { total: payout.total, earnings: payout.earnings ?? '0', currencyCode: payout.currency_code }
        : null,
  }
}

/**
 * Buckets first purchases (`isFirstPurchase`) created on/after `sinceDate` (a `YYYY-MM-DD`) into per-UTC-day
 * counts, for the daily funnel's "purchases" column, so renewals and card updates never count as a sale.
 * Returns a `Map<YYYY-MM-DD, count>`; throws on any Paddle error (the funnel source catches and shows that
 * column as dashes). Paddle's filter operators are uppercase (`[GTE]`); a lowercase one is silently ignored.
 */
export async function fetchPaddlePurchasesByDay(env: PaddleEnv, sinceDate: string): Promise<Map<string, number>> {
  const startIso = new Date(`${sinceDate}T00:00:00Z`).toISOString()
  const raw = await paddleFetchAll<PaddleRawTransaction>(
    env.PADDLE_API_KEY_LIVE,
    `${transactionsPath}&created_at[GTE]=${startIso}`,
  )
  const byDay = new Map<string, number>()
  for (const txn of raw.map(parseTransaction)) {
    if (!isFirstPurchase(txn)) continue
    // `created_at` is ISO8601 with a `Z`, so Date parses it as UTC; slice to the UTC day.
    const day = new Date(txn.createdAt).toISOString().slice(0, 10)
    byDay.set(day, (byDay.get(day) ?? 0) + 1)
  }
  return byDay
}

/**
 * Bumped whenever the cached shape changes, so an entry cached by an older build is never read. The entry
 * is the all-time list itself: every range windows it, so one key serves them all.
 */
const cacheSource = 'paddle-v3'
const cacheKey = 'all'

/** Every completed transaction, all time. The range never reaches Paddle: `#lib/money.ts` windows the list. */
export async function fetchPaddleData(env: PaddleEnv): Promise<SourceResult<PaddleData>> {
  try {
    const cached = await cacheGet<PaddleData>(cacheSource, cacheKey)
    if (cached) return { ok: true, data: cached }
    const raw = await paddleFetchAll<PaddleRawTransaction>(env.PADDLE_API_KEY_LIVE, transactionsPath)
    const data: PaddleData = { allTransactions: raw.map(parseTransaction) }
    await cacheSet(cacheSource, cacheKey, data)
    return { ok: true, data }
  } catch (e) {
    return { ok: false, error: `Paddle: ${e instanceof Error ? e.message : String(e)}` }
  }
}
