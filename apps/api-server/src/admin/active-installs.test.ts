/**
 * `/admin/active-installs`: the pure assembly (version order, interval coverage, windows, zero-fill),
 * the SQL against a REAL SQLite seeded through the `/heartbeat` INSERTs (its DAU and app hours must
 * match `/admin/heartbeat-dau`, its new installs `/admin/funnel`), and the route's auth and validation.
 */
import { describe, expect, it } from 'vitest'
import { app } from '../index'
import { createSqliteD1, type SqliteD1 } from '../sqlite-d1-test-helper'
import { insertHeartbeatSql, upsertInstallSummarySql } from '../telemetry/heartbeat'
import {
  assembleActiveInstalls,
  compareVersions,
  queryActiveInstalls,
  readSinceFor,
  type ActiveInstallsResponse,
  type BeatRow,
} from './active-installs'
import { heartbeatDauSql } from './admin'
import { assembleFunnel, buildDateList, queryFunnelRows, shiftDate } from './funnel'

type DatabaseSync = SqliteD1['sqlite']

/** Ten output days ending "today", 2026-07-10. */
const today = '2026-07-10'
const dates = buildDateList(10, new Date(`${today}T12:00:00Z`))

function row(date: string, installId: string, version = '0.60.0', seconds = 3_600): BeatRow {
  return { date, installId, version, seconds }
}

function day(response: ActiveInstallsResponse, date: string) {
  const found = response.days.find((d) => d.date === date)
  if (!found) throw new Error(`no day ${date}`)
  return found
}

describe('compareVersions', () => {
  it('orders by number, never by string', () => {
    expect(['0.9.0', '0.61.0', '0.10.2', '0.61.1'].sort(compareVersions)).toEqual([
      '0.9.0',
      '0.10.2',
      '0.61.0',
      '0.61.1',
    ])
  })

  it('puts a pre-release below its release and orders pre-releases by number', () => {
    expect(['0.62.0', '0.62.0-beta.10', '0.61.9', '0.62.0-beta.2'].sort(compareVersions)).toEqual([
      '0.61.9',
      '0.62.0-beta.2',
      '0.62.0-beta.10',
      '0.62.0',
    ])
  })

  it('is a total order: equal only for the same string', () => {
    expect(compareVersions('1.0.0', '1.0.0')).toBe(0)
    expect(compareVersions('1.0.01', '1.0.1')).not.toBe(0)
    expect(Math.sign(compareVersions('1.0.01', '1.0.1'))).toBe(-Math.sign(compareVersions('1.0.1', '1.0.01')))
    expect(Number.isNaN(compareVersions('dev', '1.0.0'))).toBe(false)
  })
})

describe('assembleActiveInstalls', () => {
  it('zero-fills every output day, oldest first, today included', () => {
    const response = assembleActiveInstalls([], [], dates)
    expect(response.days.map((d) => d.date)).toEqual(dates)
    expect(response.days.at(-1)?.date).toBe(today)
    expect(response.days[0]).toEqual({
      date: dates[0],
      dau: 0,
      newInstalls: 0,
      appHours: 0,
      wau: 0,
      mau: 0,
      byVersion: {},
    })
  })

  it('counts DAU, app hours, and new installs per day', () => {
    const rows = [
      row('2026-07-05', 'a', '0.60.0', 7_200),
      row('2026-07-05', 'b', '0.60.0', 1_800),
      row('2026-07-06', 'a', '0.60.0', 3_600),
    ]
    const response = assembleActiveInstalls(rows, [{ date: '2026-07-05', count: 1 }], dates)
    expect(day(response, '2026-07-05')).toMatchObject({ dau: 2, appHours: 2.5, newInstalls: 1 })
    expect(day(response, '2026-07-06')).toMatchObject({ dau: 1, appHours: 1, newInstalls: 0 })
  })

  it("counts an install under its highest version that day, so a day's versions sum to its DAU", () => {
    const rows = [
      row('2026-07-05', 'a', '0.9.0'),
      row('2026-07-05', 'a', '0.10.0'),
      row('2026-07-05', 'b', '0.10.0-beta.1'),
      row('2026-07-05', 'b', '0.9.0'),
    ]
    const response = assembleActiveInstalls(rows, [], dates)
    const d = day(response, '2026-07-05')
    expect(d.byVersion).toEqual({ '0.10.0': 1, '0.10.0-beta.1': 1 })
    expect(Object.values(d.byVersion).reduce((s, n) => s + n, 0)).toBe(d.dau)
  })

  it('covers seven days for WAU and 28 for MAU from each beat, merging overlaps per install', () => {
    // Beats on 07-01 and 07-03: WAU covers 07-01..07-09 (two overlapping intervals merged, counted once).
    const rows = [row('2026-07-01', 'a'), row('2026-07-03', 'a')]
    const response = assembleActiveInstalls(rows, [], dates)
    expect(response.days.map((d) => [d.date, d.wau])).toEqual([
      ['2026-07-01', 1],
      ['2026-07-02', 1],
      ['2026-07-03', 1],
      ['2026-07-04', 1],
      ['2026-07-05', 1],
      ['2026-07-06', 1],
      ['2026-07-07', 1],
      ['2026-07-08', 1],
      ['2026-07-09', 1],
      ['2026-07-10', 0],
    ])
    expect(response.days.every((d) => d.mau === 1)).toBe(true)
  })

  it('reads coverage from beats before the output range, clipped to it', () => {
    // A beat 27 days before the last day still counts toward that day's MAU, not one day later.
    const rows = [row(shiftDate(today, -27), 'old'), row(shiftDate(today, -28), 'older')]
    const response = assembleActiveInstalls(rows, [], dates)
    expect(day(response, today).mau).toBe(1)
    expect(day(response, shiftDate(today, -1)).mau).toBe(2)
    expect(response.days.every((d) => d.dau === 0 && d.wau === 0)).toBe(true)
  })

  it("clips coverage at today, so yesterday's beat never spills past the range", () => {
    const rows = [row(shiftDate(today, -1), 'a')]
    const response = assembleActiveInstalls(rows, [], dates)
    expect(response.days.length).toBe(dates.length)
    expect(day(response, today)).toMatchObject({ wau: 1, mau: 1, dau: 0 })
  })

  it('counts two installs on separate days once each in WAU', () => {
    const rows = [row('2026-07-08', 'a'), row('2026-07-09', 'b'), row('2026-07-09', 'a')]
    const response = assembleActiveInstalls(rows, [], dates)
    expect(day(response, today).wau).toBe(2)
  })

  it('builds 7/30/90-day windows of complete days ending yesterday', () => {
    const rows = [
      row(today, 'today-only'),
      row(shiftDate(today, -1), 'a', '0.61.0'),
      row(shiftDate(today, -3), 'a', '0.60.0'),
      row(shiftDate(today, -20), 'b', '0.59.0'),
      row(shiftDate(today, -90), 'c', '0.50.0'),
      row(shiftDate(today, -91), 'd', '0.49.0'),
    ]
    const { windows } = assembleActiveInstalls(rows, [], dates)
    expect(windows).toEqual([
      {
        days: 7,
        since: shiftDate(today, -7),
        until: shiftDate(today, -1),
        installs: 1,
        byVersion: { '0.61.0': 1, '0.60.0': 1 },
      },
      {
        days: 30,
        since: shiftDate(today, -30),
        until: shiftDate(today, -1),
        installs: 2,
        byVersion: { '0.61.0': 1, '0.60.0': 1, '0.59.0': 1 },
      },
      {
        days: 90,
        since: shiftDate(today, -90),
        until: shiftDate(today, -1),
        installs: 3,
        byVersion: { '0.61.0': 1, '0.60.0': 1, '0.59.0': 1, '0.50.0': 1 },
      },
    ])
  })

  it('never puts an install id in the response', () => {
    const response = assembleActiveInstalls([row('2026-07-05', 'anal_secret')], [], dates)
    expect(JSON.stringify(response)).not.toContain('anal_secret')
  })
})

describe('readSinceFor', () => {
  it("reaches 27 days before the first output day, for that day's MAU", () => {
    const long = buildDateList(181, new Date(`${today}T12:00:00Z`))
    expect(readSinceFor(long)).toBe(shiftDate(long[0], -27))
  })

  it('reaches 90 days back for a short range, so the 90-day window stays whole', () => {
    expect(readSinceFor(dates)).toBe(shiftDate(today, -90))
  })
})

function beat(db: DatabaseSync, id: string, at: string, version = '0.60.0', uptime: number | null = null): void {
  db.prepare(insertHeartbeatSql).run(id, version, '26.0', 'aarch64', 'release', null, uptime, null, null, at)
  db.prepare(upsertInstallSummarySql).run(id, at, null, null)
}

function seeded(): SqliteD1 {
  const sqlite = createSqliteD1()
  const db = sqlite.sqlite
  beat(db, 'anal_old', '2026-05-01 09:00:00', '0.50.0') // first beat long before the range
  beat(db, 'anal_old', '2026-07-02 09:00:00', '0.60.0', 10_800)
  beat(db, 'anal_a', '2026-07-02 10:00:00', '0.60.0')
  beat(db, 'anal_a', '2026-07-02 23:59:59', '0.61.0', 1_800)
  beat(db, 'anal_a', '2026-07-09 08:00:00', '0.61.0', 10_800)
  beat(db, 'anal_b', '2026-07-09 00:00:00', '0.61.0', 3_600)
  beat(db, 'anal_c', '2026-07-10 00:00:00', '0.61.0', 600)
  return sqlite
}

describe('the active-installs queries against real SQLite', () => {
  it('matches /admin/heartbeat-dau for DAU and app hours, day by day', async () => {
    const { d1, sqlite } = seeded()
    const response = await queryActiveInstalls(d1, dates)
    const reference = sqlite.prepare(heartbeatDauSql('')).all() as unknown as Array<{
      date: string
      dau: number
      appHours: number
    }>
    for (const ref of reference.filter((r) => r.date >= dates[0])) {
      expect(day(response, ref.date)).toMatchObject({ dau: ref.dau, appHours: ref.appHours })
    }
    expect(day(response, '2026-07-03')).toMatchObject({ dau: 0, appHours: 0 })
  })

  it('matches /admin/funnel for new installs, day by day', async () => {
    const { d1 } = seeded()
    const response = await queryActiveInstalls(d1, dates)
    const now = new Date(`${today}T12:00:00Z`)
    const funnelRows = await queryFunnelRows(d1, { sinceDate: dates[0], today, country: null })
    const funnel = assembleFunnel(dates, funnelRows, null, now)
    expect(response.days.map((d) => d.newInstalls)).toEqual(funnel.map((d) => d.newInstalls))
    expect(day(response, '2026-07-02').newInstalls).toBe(1)
  })

  it('reads versions per install-day, and older beats for WAU, MAU, and the windows', async () => {
    const { d1 } = seeded()
    const response = await queryActiveInstalls(d1, dates)
    expect(day(response, '2026-07-02').byVersion).toEqual({ '0.60.0': 1, '0.61.0': 1 })
    expect(day(response, today)).toMatchObject({ dau: 1, wau: 3, mau: 4 })
    const ninety = response.windows.find((w) => w.days === 90)
    expect(ninety).toMatchObject({ installs: 3, byVersion: { '0.50.0': 1, '0.60.0': 2, '0.61.0': 2 } })
  })
})

describe('GET /admin/active-installs', () => {
  const bindings = (d1: D1Database) => ({ TELEMETRY_DB: d1, ADMIN_API_TOKEN: 'test-admin-token-secret' })
  const authHeaders = { Authorization: 'Bearer test-admin-token-secret' }

  it('requires the admin token', async () => {
    const res = await app.request('/admin/active-installs', {}, bindings(createSqliteD1().d1))
    expect(res.status).toBe(401)
  })

  it('rejects days it has no window for', async () => {
    for (const days of ['0', '401', 'abc', '7abc']) {
      const res = await app.request(
        `/admin/active-installs?days=${days}`,
        { headers: authHeaders },
        bindings(createSqliteD1().d1),
      )
      expect(res.status).toBe(400)
    }
  })

  it('defaults to 181 days ending today, plus three windows', async () => {
    const res = await app.request('/admin/active-installs', { headers: authHeaders }, bindings(createSqliteD1().d1))
    expect(res.status).toBe(200)
    const body: ActiveInstallsResponse = await res.json()
    expect(body.days).toHaveLength(181)
    expect(body.days.at(-1)?.date).toBe(new Date().toISOString().slice(0, 10))
    expect(body.windows.map((w) => w.days)).toEqual([7, 30, 90])
  })
})
