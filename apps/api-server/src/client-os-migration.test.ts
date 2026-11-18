import { describe, expect, it } from 'vitest'
import { handleRetentionSweep } from './scheduled'
import { applyMigrationsFrom, createSqliteD1 } from './sqlite-d1-test-helper'
import type { Bindings } from './types'

/** Migration 0033 against the schema it meets in production, on a real SQLite. */
describe('migration 0033 (client OS)', () => {
  function seededBefore() {
    const { sqlite } = createSqliteD1({ before: '0033' })
    sqlite.exec(`INSERT INTO downloads (app_version, arch, country, continent, hashed_ip, source)
                 VALUES ('0.61.0', 'x86_64', 'SE', 'EU', 'h1', 'website')`)
    sqlite.exec(`INSERT INTO downloads_daily_unique (date, app_version, arch, country, source, unique_downloaders)
                 VALUES ('2026-08-01', '0.55.0', 'x86_64', 'SE', 'website', 7)`)
    applyMigrationsFrom(sqlite, '0033')
    return sqlite
  }

  it('reads every existing download as a macOS DMG', () => {
    const sqlite = seededBefore()
    expect(sqlite.prepare('SELECT os, kind FROM downloads').all()).toEqual([{ os: 'macos', kind: 'dmg' }])
  })

  it('keeps every rollup row, as macOS', () => {
    const sqlite = seededBefore()
    expect(
      sqlite.prepare('SELECT date, app_version, os, arch, unique_downloaders FROM downloads_daily_unique').all(),
    ).toEqual([{ date: '2026-08-01', app_version: '0.55.0', os: 'macos', arch: 'x86_64', unique_downloaders: 7 }])
  })

  it('keeps a Mac and a Windows x86_64 rollup row of the same day apart, where the old key merged them', () => {
    const sqlite = seededBefore()
    const insert =
      sqlite.prepare(`INSERT OR IGNORE INTO downloads_daily_unique (date, app_version, os, arch, country, source, unique_downloaders)
                                   VALUES ('2026-08-01', '0.55.0', ?, 'x86_64', 'SE', 'website', 3)`)
    insert.run('windows')
    insert.run('macos')
    expect(sqlite.prepare('SELECT os, unique_downloaders FROM downloads_daily_unique ORDER BY os').all()).toEqual([
      { os: 'macos', unique_downloaders: 7 },
      { os: 'windows', unique_downloaders: 3 },
    ])
  })

  it("still takes an older Worker's inserts, which name no os, as macOS", () => {
    const sqlite = seededBefore()
    sqlite.exec(`INSERT INTO downloads (app_version, arch, country, continent, hashed_ip, source, ua_family)
                 VALUES ('0.62.0', 'aarch64', 'HU', 'EU', 'h2', 'other', 'human')`)
    sqlite.exec(`INSERT OR IGNORE INTO downloads_daily_unique (date, app_version, arch, country, source, unique_downloaders)
                 VALUES ('2026-08-02', '0.62.0', 'aarch64', 'HU', 'other', 1)`)
    expect(sqlite.prepare("SELECT os, kind FROM downloads WHERE app_version = '0.62.0'").get()).toEqual({
      os: 'macos',
      kind: 'dmg',
    })
    expect(sqlite.prepare("SELECT os FROM downloads_daily_unique WHERE date = '2026-08-02'").get()).toEqual({
      os: 'macos',
    })
  })

  it('adds a nullable os to the heartbeat, feedback, and both report tables', () => {
    const sqlite = seededBefore()
    for (const table of ['heartbeat', 'feedback', 'crash_reports', 'error_reports']) {
      const column = sqlite
        .prepare(`SELECT "notnull" AS required, dflt_value AS defaultValue FROM pragma_table_info(?) WHERE name = 'os'`)
        .get(table)
      expect(column, table).toEqual({ required: 0, defaultValue: null })
    }
  })
})

describe('the retention sweep on downloads of two OSes', () => {
  it('captures a separate unique count per OS for one day, arch, country, and source', async () => {
    const { d1, sqlite } = createSqliteD1()
    const old = new Date(Date.now() - 95 * 86_400_000).toISOString().slice(0, 19).replace('T', ' ')
    const insert =
      sqlite.prepare(`INSERT INTO downloads (created_at, app_version, os, kind, arch, country, continent, hashed_ip, source)
                                   VALUES (?, '0.70.0', ?, ?, 'x86_64', 'SE', 'EU', ?, 'website')`)
    insert.run(old, 'macos', 'dmg', 'h1')
    insert.run(old, 'macos', 'dmg', 'h2')
    insert.run(old, 'windows', 'nsis', 'h3')

    await handleRetentionSweep({ TELEMETRY_DB: d1 } as Bindings)

    expect(sqlite.prepare('SELECT os, unique_downloaders FROM downloads_daily_unique ORDER BY os').all()).toEqual([
      { os: 'macos', unique_downloaders: 2 },
      { os: 'windows', unique_downloaders: 1 },
    ])
  })
})
