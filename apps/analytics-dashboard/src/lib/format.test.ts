import { describe, it, expect } from 'vitest'
import {
  changeTone,
  formatAverage,
  formatHoursChange,
  formatMultiple,
  formatMultipleChange,
  formatPercent,
  formatPointChange,
  formatRelativeChange,
  formatUtcDateTime,
} from './format.js'

describe('rate and change formatters', () => {
  it('formats averages with a decimal only while small', () => {
    expect(formatAverage(12.345)).toBe('12.3')
    expect(formatAverage(1234.5)).toBe('1,235')
  })

  it('formats percents, including past 100%', () => {
    expect(formatPercent(0.0456)).toBe('4.6%')
    expect(formatPercent(0.456)).toBe('46%')
    expect(formatPercent(1.35)).toBe('135%')
  })

  it('formats multiples', () => {
    expect(formatMultiple(3.21)).toBe('3.2×')
    expect(formatMultiple(14.6)).toBe('15×')
  })

  it('signs changes with a real minus and a "±0" for no change', () => {
    expect(formatRelativeChange(0.5)).toBe('+50%')
    expect(formatRelativeChange(-0.123)).toBe('−12%')
    expect(formatRelativeChange(0.001)).toBe('±0%')
    expect(formatPointChange(0.0123)).toBe('+1.2 pp')
    expect(formatPointChange(-0.05)).toBe('−5 pp')
    expect(formatMultipleChange(0.44)).toBe('+0.4×')
    expect(formatHoursChange(-0.26)).toBe('−0.3 h')
    expect(formatHoursChange(0.04)).toBe('±0 h')
  })
})

describe('changeTone', () => {
  it('colors a change by its sign, flipped when a rise is bad news', () => {
    expect(changeTone(0.2, formatRelativeChange)).toBe('good')
    expect(changeTone(-0.2, formatRelativeChange)).toBe('bad')
    expect(changeTone(0.2, formatRelativeChange, false)).toBe('bad')
  })

  // Regression anchor: a +0.3% change printed "±0%" in green, from the raw sign.
  it('keeps a change that displays as zero neutral, whatever its raw sign', () => {
    expect(formatRelativeChange(0.003)).toBe('±0%')
    expect(changeTone(0.003, formatRelativeChange)).toBe('neutral')
    expect(changeTone(-0.004, formatRelativeChange)).toBe('neutral')
    expect(changeTone(0.0004, formatPointChange)).toBe('neutral')
    expect(changeTone(-0.04, formatMultipleChange)).toBe('neutral')
    expect(changeTone(0.04, formatHoursChange)).toBe('neutral')
  })

  it('is neutral for an unknown change', () => {
    expect(changeTone(null, formatRelativeChange)).toBe('neutral')
  })
})

describe('formatUtcDateTime', () => {
  it('prints an ISO timestamp as a UTC day and time', () => {
    expect(formatUtcDateTime('2026-09-16T10:04:02.000Z')).toBe('2026-09-16 10:04')
  })

  it('stays on UTC for a timestamp written in another offset', () => {
    expect(formatUtcDateTime('2026-09-16T23:30:00+02:00')).toBe('2026-09-16 21:30')
  })

  it('returns an unparseable timestamp verbatim', () => {
    expect(formatUtcDateTime('not a date')).toBe('not a date')
  })
})
