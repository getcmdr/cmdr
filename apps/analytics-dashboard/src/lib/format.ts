/** Client-safe formatting helpers shared across the dashboard's sections. */

const regionNames = new Intl.DisplayNames(['en'], { type: 'region' })

/** A country code as "Name (CC)", or the raw code when the region name is unknown. */
export function formatCountry(code: string): string {
  try {
    const upper = code.toUpperCase()
    const name = regionNames.of(upper)
    return name && name !== upper ? `${name} (${upper})` : code
  } catch {
    return code
  }
}

const osNames: Record<string, string> = { macos: 'macOS', windows: 'Windows', linux: 'Linux' }

/** An OS as the api-server spells it (`macos`) the way people write it (`macOS`); an unknown one stays as it came. */
export function formatOs(os: string): string {
  return osNames[os] ?? os
}

/** A number with US thousands separators. */
export function formatNumber(n: number): string {
  return n.toLocaleString('en-US')
}

/** Cents (string or number) as a localized currency amount. */
export function formatCurrency(cents: string | number, currency = 'USD'): string {
  const value = Number(cents) / 100
  return new Intl.NumberFormat('en-US', { style: 'currency', currency }).format(value)
}

/** A percent delta between two values, with its sign and whether it's non-negative (for coloring). */
export function formatDelta(current: number, previous: number): { text: string; positive: boolean } {
  if (previous === 0) return { text: 'N/A', positive: true }
  const pct = ((current - previous) / previous) * 100
  const sign = pct >= 0 ? '+' : ''
  return { text: `${sign}${pct.toFixed(1)}%`, positive: pct >= 0 }
}

/** A count that may be fractional (an average), with one decimal under 100 and none above. */
export function formatAverage(n: number): string {
  return n.toLocaleString('en-US', { maximumFractionDigits: Math.abs(n) < 100 ? 1 : 0 })
}

/** A 0..1+ fraction as a percent: one decimal under 10%, none above. Rates can pass 100%. */
export function formatPercent(fraction: number): string {
  const pct = fraction * 100
  return `${pct.toLocaleString('en-US', { maximumFractionDigits: Math.abs(pct) < 10 ? 1 : 0 })}%`
}

/** A ratio as a multiple, like "3.2×". */
export function formatMultiple(ratio: number): string {
  return `${ratio.toLocaleString('en-US', { maximumFractionDigits: ratio < 10 ? 1 : 0 })}×`
}

/**
 * A signed change formatter that also exposes the rounded number it prints, so a color can follow what's
 * on screen: a change that prints "±0%" must not read green or red from its raw sign (`changeTone`).
 */
export interface ChangeFormat {
  (change: number): string
  /** The change as printed, in display units (0.004 → 0 for "±0%"). */
  shown: (change: number) => number
}

function changeFormat(shown: (change: number) => number, unit: string): ChangeFormat {
  const format = (change: number): string => {
    const value = shown(change)
    return value === 0 ? `±0${unit}` : `${value > 0 ? '+' : '−'}${String(Math.abs(value))}${unit}`
  }
  return Object.assign(format, { shown })
}

/** A relative change (0.5 = +50%), signed. Under 1% rounds to "±0%". */
export const formatRelativeChange = changeFormat((change) => Math.round(change * 100), '%')

/** An absolute change of two rates (0.012 = +1.2 percentage points), signed, in "pp". */
export const formatPointChange = changeFormat((change) => Math.round(change * 1000) / 10, ' pp')

/** An absolute change of two multiples (+0.4×), signed. */
export const formatMultipleChange = changeFormat((change) => Math.round(change * 10) / 10, '×')

/** An absolute change of two hour values (+0.4 h), signed. */
export const formatHoursChange = changeFormat((change) => Math.round(change * 10) / 10, ' h')

export type ChangeTone = 'neutral' | 'good' | 'bad'

/**
 * How a change reads: neutral when unknown or when it prints as zero, else good or bad by its sign
 * (`upIsGood` false for a metric where a rise is bad news, like the bot meter).
 */
export function changeTone(change: number | null, format: ChangeFormat, upIsGood = true): ChangeTone {
  if (change === null) return 'neutral'
  const shown = format.shown(change)
  if (shown === 0) return 'neutral'
  return shown > 0 === upIsGood ? 'good' : 'bad'
}

/** The text class for each tone. */
export const changeToneClass: Record<ChangeTone, string> = {
  neutral: 'text-text-secondary',
  good: 'text-success',
  bad: 'text-danger',
}

/** A short local time-of-day (e.g. "09:41 AM") from an ISO timestamp. */
export function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' })
}

/**
 * An ISO timestamp as `YYYY-MM-DD HH:MM` in UTC. UTC rather than local time because every other day
 * bucket on the dashboard is UTC, and a ledger row read in two zones must not look like two events.
 * An unparseable timestamp comes back verbatim, so a broken row shows what it actually holds.
 */
export function formatUtcDateTime(iso: string): string {
  const ms = Date.parse(iso)
  if (Number.isNaN(ms)) return iso
  return new Date(ms).toISOString().slice(0, 16).replace('T', ' ')
}

/** Converts daily rows ({day, views/count}) into uPlot's AlignedData format [timestamps[], values[]]. */
export function toChartData(rows: Array<{ day: string; views?: number; count?: number }>): [number[], number[]] {
  const timestamps = rows.map((r) => new Date(r.day).getTime() / 1000)
  const values = rows.map((r) => r.views ?? r.count ?? 0)
  return [timestamps, values]
}
