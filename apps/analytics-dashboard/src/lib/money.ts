/**
 * Client-safe money math over Paddle's completed transactions, windowed here from one all-time fetch
 * (`#lib/server/sources/paddle.ts`), so every figure on the page and in the report reads the same rows.
 *
 * - **Amounts stay per currency.** Each transaction counts in its payout currency when Paddle reports one
 *   (`details.payout_totals`), else in its own checkout currency, and amounts in different currencies are
 *   never added: a mixed set prints one line per currency.
 * - **Revenue is gross of refunds.** A refunded purchase stays `completed` in Paddle (a refund is an
 *   adjustment), so it still counts here.
 * - **Stats cover complete days only** (today is partial); only the purchases list shows today.
 * - Amounts are in the currency's minor units (cents), as Paddle sends them.
 */
import { shiftDay } from './time-range.js'

/** One completed Paddle transaction, as `#lib/server/sources/paddle.ts` parses it. */
export interface PaddleTransaction {
  id: string
  status: string
  createdAt: string
  /** The checkout total in `currencyCode`'s minor units. */
  total: string
  currencyCode: string
  /** Paddle's `origin` (`web`, `api`, `subscription_recurring`, …), or null when it sent none. */
  origin: string | null
  /** The same transaction in the payout currency (`details.payout_totals`), or null when Paddle sent none. */
  payout: { total: string; earnings: string; currencyCode: string } | null
}

/**
 * The `origin`s of a first purchase: the checkout (`web`) or an API-created transaction (`api`). An
 * allowlist on purpose: renewals (`subscription_recurring`), card updates
 * (`subscription_payment_method_change`, often a zero-value completed transaction), plan changes
 * (`subscription_update`), and one-off charges (`subscription_charge`) aren't a new paying user, and an
 * origin Paddle adds later isn't either until we say so. Paddle docs, transaction `origin`, 2026-11-17.
 */
const firstPurchaseOrigins: ReadonlySet<string> = new Set(['web', 'api'])

/** Whether a transaction is a first purchase (a new paid license), by its Paddle `origin`. */
export function isFirstPurchase(transaction: Pick<PaddleTransaction, 'origin'>): boolean {
  return transaction.origin !== null && firstPurchaseOrigins.has(transaction.origin)
}

/** An amount in one currency's minor units. */
export interface CurrencyAmount {
  currencyCode: string
  amount: number
}

/** One currency's revenue in a window, against the window before it. */
export interface RevenueLine extends CurrencyAmount {
  previousAmount: number
  /** Relative change (0.5 = +50%), or null when the previous window took nothing in this currency. */
  change: number | null
}

/** A first purchase, for the purchases list. */
export interface Purchase {
  id: string
  createdAt: string
  amount: CurrencyAmount
  /** Bought today (UTC): listed, but in no stat. */
  isToday: boolean
}

/** The UTC day a transaction was created on. */
function dayOf(transaction: PaddleTransaction): string {
  return new Date(transaction.createdAt).toISOString().slice(0, 10)
}

/** What a transaction counts as: its payout amount when Paddle reported one, else its checkout total. */
function amountOf(transaction: PaddleTransaction): CurrencyAmount {
  return transaction.payout
    ? { currencyCode: transaction.payout.currencyCode, amount: Number(transaction.payout.total) }
    : { currencyCode: transaction.currencyCode, amount: Number(transaction.total) }
}

function totalsByCurrency(transactions: PaddleTransaction[]): Map<string, number> {
  const totals = new Map<string, number>()
  for (const transaction of transactions) {
    const { currencyCode, amount } = amountOf(transaction)
    totals.set(currencyCode, (totals.get(currencyCode) ?? 0) + amount)
  }
  return totals
}

/** One line per currency, biggest first. */
export function sumByCurrency(transactions: PaddleTransaction[]): CurrencyAmount[] {
  return [...totalsByCurrency(transactions)]
    .map(([currencyCode, amount]) => ({ currencyCode, amount }))
    .sort((a, b) => b.amount - a.amount || a.currencyCode.localeCompare(b.currencyCode))
}

/** Transactions created on days in [start, end). */
function between(transactions: PaddleTransaction[], start: string, end: string): PaddleTransaction[] {
  return transactions.filter((transaction) => {
    const day = dayOf(transaction)
    return day >= start && day < end
  })
}

/**
 * Revenue over the `days` complete days before `today`, per currency, against the `days` before that.
 * Lists every currency either window took money in, biggest current amount first.
 */
export function revenueInPeriod(transactions: PaddleTransaction[], days: number, today: string): RevenueLine[] {
  const start = shiftDay(today, -days)
  const current = totalsByCurrency(between(transactions, start, today))
  const previous = totalsByCurrency(between(transactions, shiftDay(start, -days), start))
  return [...new Set([...current.keys(), ...previous.keys()])]
    .map((currencyCode) => {
      const amount = current.get(currencyCode) ?? 0
      const previousAmount = previous.get(currencyCode) ?? 0
      return { currencyCode, amount, previousAmount, change: previousAmount > 0 ? amount / previousAmount - 1 : null }
    })
    .sort((a, b) => b.amount - a.amount || a.currencyCode.localeCompare(b.currencyCode))
}

/** All revenue through yesterday, per currency. */
export function allTimeRevenue(transactions: PaddleTransaction[], today: string): CurrencyAmount[] {
  return sumByCurrency(transactions.filter((transaction) => dayOf(transaction) < today))
}

/** Paid licenses: first purchases created before `end` (an exclusive UTC day), cumulative. */
export function paidLicensesThrough(transactions: PaddleTransaction[], end: string): number {
  return transactions.filter((transaction) => isFirstPurchase(transaction) && dayOf(transaction) < end).length
}

/** The first purchases of the last `days` complete days plus today, newest first. */
export function recentPurchases(transactions: PaddleTransaction[], days: number, today: string): Purchase[] {
  const start = shiftDay(today, -days)
  return transactions
    .filter((transaction) => isFirstPurchase(transaction) && dayOf(transaction) >= start)
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
    .map((transaction) => ({
      id: transaction.id,
      createdAt: transaction.createdAt,
      amount: amountOf(transaction),
      isToday: dayOf(transaction) === today,
    }))
}
