import { describe, it, expect } from 'vitest'
import { cell, countStatsByWindow, dailyRatios, ratioStatsByWindow, rollingComplete, viewRange } from './trend-row.js'
import { addDays } from './acquisition-stats.js'

const today = '2026-11-17'

function daysBack(n: number): string[] {
  return Array.from({ length: n }, (_, i) => addDays(today, i - n + 1))
}

describe('rollingComplete', () => {
  it('gives today no rolling point, and pools the week before it', () => {
    const dates = daysBack(8)
    const points = dates.map((date) => ({ date, num: 1, den: 2 }))
    const line = rollingComplete(points, today)
    expect(line.at(-1)).toBeNull()
    expect(line.at(-2)).toBe(0.5)
    expect(line[5]).toBeNull()
  })
})

describe('dailyRatios', () => {
  it('reads unknown for a zero denominator or a missing side', () => {
    expect(
      dailyRatios([
        { date: 'a', num: 1, den: 4 },
        { date: 'b', num: 1, den: 0 },
        { date: 'c', num: null, den: 3 },
      ]),
    ).toEqual([0.25, null, null])
  })
})

describe('viewRange', () => {
  const dates = daysBack(10)

  it('picks the days of the window', () => {
    expect(viewRange(dates, { start: dates[7], end: today, current: true })).toEqual({ start: 7, stop: 10 })
  })

  it('is empty for a window outside the history', () => {
    expect(viewRange(dates, { start: addDays(today, 5), end: addDays(today, 9), current: false })).toEqual({
      start: 10,
      stop: 10,
    })
  })
})

describe('the stats grids', () => {
  it('cover each window, leaving today out', () => {
    const dates = daysBack(181)
    const counts = dates.map((date) => ({ date, value: date === today ? 1000 : 2 }))
    const stats = countStatsByWindow(counts, today)
    expect(stats[7].average).toBe(2)
    expect(stats[90].change).toBe(0)
    const ratios = dates.map((date) => ({ date, num: 1, den: 4 }))
    expect(ratioStatsByWindow(ratios, today)[30].average).toBe(0.25)
  })
})

describe('cell', () => {
  it('prints a dash for unknown and keeps a real zero', () => {
    expect(cell(null)).toBe('–')
    expect(cell(0)).toBe('0')
    expect(cell(1234)).toBe('1,234')
  })
})
