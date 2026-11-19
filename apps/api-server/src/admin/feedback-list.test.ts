/**
 * The admin feedback list, end to end through the Hono app against a REAL SQLite with every
 * migration applied, so the columns and the SQL are what's under test.
 */
import { describe, expect, it } from 'vitest'
import { app } from '../index'
import { createSqliteD1 } from '../sqlite-d1-test-helper'

const authHeaders = { Authorization: 'Bearer test-admin-token-secret', 'Content-Type': 'application/json' }

function setup() {
  const { d1, sqlite } = createSqliteD1()
  sqlite
    .prepare(
      `INSERT INTO feedback (id, created_at, feedback, email, app_version, os_version, build_mode)
         VALUES (?, datetime('now'), ?, ?, '0.52.0', 'macOS 26.0', 'release')`,
    )
    .run(1, 'Cyrillic quick filter', 'someone@example.com')
  const bindings = { TELEMETRY_DB: d1, ADMIN_API_TOKEN: 'test-admin-token-secret' }
  return { sqlite, bindings }
}

describe('GET /admin/feedback', () => {
  it('returns the linked error report id, and null when the message came without logs', async () => {
    const { sqlite, bindings } = setup()
    sqlite
      .prepare(
        `INSERT INTO feedback (id, created_at, feedback, app_version, os_version, error_report_id)
           VALUES (2, datetime('now', '+1 minute'), 'Copy stalls', '0.52.0', 'macOS 26.0', 'ERR-AB7PV')`,
      )
      .run()
    const res = await app.request('/admin/feedback?range=7d', { headers: authHeaders }, bindings)
    const rows = await res.json<Array<{ id: number; errorReportId: string | null }>>()
    expect(rows.map((row) => [row.id, row.errorReportId])).toEqual([
      [2, 'ERR-AB7PV'],
      [1, null],
    ])
  })

  it('returns the answered time the board sync stamped, and null while a reply is owed', async () => {
    const { sqlite, bindings } = setup()
    const listed = async () => {
      const res = await app.request('/admin/feedback?range=7d', { headers: authHeaders }, bindings)
      return (await res.json<Array<{ answeredAt: string | null }>>())[0]?.answeredAt
    }
    expect(await listed()).toBeNull()
    sqlite.prepare(`UPDATE feedback SET answered_at = '2026-10-14 21:12:00' WHERE id = 1`).run()
    expect(await listed()).toBe('2026-10-14 21:12:00')
  })
})
