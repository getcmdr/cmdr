import { describe, it, expect } from 'vitest'
import type { VersionWindow } from './server/sources/active-installs.js'
import { errorsPer100, errorsSinceRelease, type ErrorReportRow } from './feedback-and-errors.js'

function report(id: string, appVersion: string, date: string, generatedAt = `${date}T12:00:00.000Z`): ErrorReportRow {
  return { id, kind: 'auto', appVersion, osVersion: '15.5', arch: 'aarch64', date, generatedAt }
}

const window: VersionWindow = {
  days: 7,
  since: '2026-08-03',
  until: '2026-08-09',
  installs: 30,
  byVersion: { '0.61.0': 20, '0.60.0': 12, '0.9.0': 3 },
}

describe('errorsPer100', () => {
  it("divides the window's reports by the installs that ran each version, newest version first", () => {
    const rows = [
      report('a', '0.61.0', '2026-08-03'),
      report('b', '0.61.0', '2026-08-09'),
      report('c', '0.61.0', '2026-08-10'), // today: outside the complete-day window
      report('d', '0.61.0', '2026-08-02'), // before it
      report('e', '0.60.0', '2026-08-05'),
      report('f', '0.9.0', '2026-08-05'),
    ]
    expect(errorsPer100(rows, window)).toEqual([
      { version: '0.61.0', reports: 2, installs: 20, per100: 10 },
      { version: '0.60.0', reports: 1, installs: 12, per100: 100 / 12 },
      { version: '0.9.0', reports: 1, installs: 3, per100: null },
    ])
  })

  it('lists a version that reported errors without any heartbeat, with no rate', () => {
    expect(errorsPer100([report('a', '0.58.0', '2026-08-04')], { ...window, byVersion: {} })).toEqual([
      { version: '0.58.0', reports: 1, installs: 0, per100: null },
    ])
  })
})

describe('errorsSinceRelease', () => {
  it('counts reports generated at or after the publish time', () => {
    const rows = [
      report('a', '0.61.0', '2026-08-01', '2026-08-01T09:59:59.000Z'),
      report('b', '0.61.0', '2026-08-01', '2026-08-01T10:00:00.000Z'),
      report('c', '0.61.0', '2026-08-05'),
    ]
    expect(errorsSinceRelease(rows, '2026-08-01T10:00:00Z')).toBe(2)
  })
})
