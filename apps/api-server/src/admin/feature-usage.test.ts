/**
 * `/admin/feature-usage` against a REAL SQLite with every migration, fed through the same INSERTs
 * `/heartbeat` runs, plus the mapping's own invariants: every id is a real `feature-status.json` feature
 * and no stored event can count toward two features.
 */
/* eslint-disable-next-line no-restricted-imports -- Reads the repo-root feature-status.json, Node-side. */
import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { app } from '../index'
import { createSqliteD1, type SqliteD1 } from '../sqlite-d1-test-helper'
import { insertEventsSql, insertHeartbeatSql, upsertInstallSummarySql } from '../telemetry/heartbeat'
import {
  featureUsageDefinitions,
  featureUsageSql,
  queryFeatureUsage,
  seenEventsSql,
  type FeatureUsage,
  type FeatureUsageResponse,
} from './feature-usage'

type DatabaseSync = SqliteD1['sqlite']

function beat(db: DatabaseSync, id: string, at: string): void {
  db.prepare(insertHeartbeatSql).run(id, '1.2.3', '26.0', 'aarch64', 'release', null, null, null, null, at, 'macos')
  db.prepare(upsertInstallSummarySql).run(id, at, null, null)
}

let eventSeq = 0

function event(db: DatabaseSync, id: string, name: string, occurredAt: string, props: object = {}): void {
  eventSeq += 1
  const row = [name, occurredAt, JSON.stringify(props), '1.2.3', `evt-${String(eventSeq)}`]
  db.prepare(insertEventsSql).run(id, JSON.stringify([row]))
}

/** The window under test starts here; anything earlier is outside it but still "ever seen". */
const since = new Date('2026-11-01T00:00:00.000Z')

function seeded(): SqliteD1 {
  const sqlite = createSqliteD1()
  const db = sqlite.sqlite
  beat(db, 'anal_a', '2026-11-02 09:00:00')
  beat(db, 'anal_b', '2026-11-03 09:00:00')
  beat(db, 'anal_c', '2026-11-04 09:00:00')
  beat(db, 'anal_old', '2026-10-20 09:00:00') // active before the window only

  // Archives: a pane inside a zip (two installs) plus an edit; a local pane counts toward nothing.
  event(db, 'anal_a', 'pane_navigated', '2026-11-02T09:01:00.000Z', { volume_kind: 'archive' })
  event(db, 'anal_a', 'pane_navigated', '2026-11-02T09:02:00.000Z', { volume_kind: 'archive' })
  event(db, 'anal_b', 'pane_navigated', '2026-11-03T09:01:00.000Z', { volume_kind: 'archive' })
  event(db, 'anal_b', 'archive_edit_completed', '2026-11-03T09:02:00.000Z', { item_count: '1' })
  event(db, 'anal_c', 'pane_navigated', '2026-11-04T09:01:00.000Z', { volume_kind: 'local' })

  // Ask Cmdr: split by outcome.
  event(db, 'anal_a', 'ask_cmdr_turn', '2026-11-02T10:00:00.000Z', { outcome: 'answered' })
  event(db, 'anal_a', 'ask_cmdr_turn', '2026-11-02T10:01:00.000Z', { outcome: 'answered' })
  event(db, 'anal_c', 'ask_cmdr_turn', '2026-11-04T10:00:00.000Z', { outcome: 'failed' })

  // AI suggestions: a rename group is AI rename, a move group is AI file organization, a server
  // group is neither.
  event(db, 'anal_a', 'suggestion_group_proposed', '2026-11-02T10:02:00.000Z', { verb: 'rename' })
  event(db, 'anal_a', 'suggestion_group_approved', '2026-11-02T10:03:00.000Z', { verb: 'rename' })
  event(db, 'anal_c', 'suggestion_group_proposed', '2026-11-04T10:02:00.000Z', { verb: 'move' })
  event(db, 'anal_c', 'suggestion_group_proposed', '2026-11-04T10:03:00.000Z', { verb: 'add_server' })

  // Search: one run inside the window, one before it.
  event(db, 'anal_b', 'search_used', '2026-11-03T11:00:00.000Z', { mode: 'regex' })
  event(db, 'anal_old', 'search_used', '2026-10-20T11:00:00.000Z', { mode: 'filename' })

  // Tabs: only ever seen before the window, so it reads a real zero, not "no data yet".
  event(db, 'anal_old', 'tab_opened', '2026-10-20T12:00:00.000Z')
  return sqlite
}

function byId(features: FeatureUsage[], id: string): FeatureUsage {
  const feature = features.find((f) => f.id === id)
  if (!feature) throw new Error(`no feature ${id}`)
  return feature
}

describe('the feature-usage queries against real SQLite', () => {
  it('counts distinct installs and events per feature, inside the window only', async () => {
    const { d1 } = seeded()
    const { activeInstalls, features } = await queryFeatureUsage(d1, { since })

    expect(activeInstalls).toBe(3)
    expect(byId(features, 'archives')).toMatchObject({
      installs: 2,
      events: 4,
      everSeen: true,
      parts: [
        { key: 'pane_navigated', installs: 2, events: 3 },
        { key: 'archive_edit_completed', installs: 1, events: 1 },
      ],
    })
    expect(byId(features, 'search')).toMatchObject({
      installs: 1,
      events: 1,
      parts: [{ key: 'regex', installs: 1, events: 1 }],
    })
  })

  it('splits Ask Cmdr by outcome', async () => {
    const { d1 } = seeded()
    const { features } = await queryFeatureUsage(d1, { since })
    expect(byId(features, 'ask-cmdr')).toMatchObject({
      installs: 2,
      events: 3,
      parts: [
        { key: 'answered', installs: 1, events: 2 },
        { key: 'failed', installs: 1, events: 1 },
      ],
    })
  })

  it('tells AI rename from AI file organization by verb, and counts a server group as neither', async () => {
    const { d1 } = seeded()
    const { features } = await queryFeatureUsage(d1, { since })
    expect(byId(features, 'ai-rename')).toMatchObject({ installs: 1, events: 2 })
    expect(byId(features, 'ai-batch')).toMatchObject({
      installs: 1,
      events: 1,
      parts: [{ key: 'suggestion_group_proposed', installs: 1, events: 1 }],
    })
  })

  it('separates "never stored" from "a real zero in this window"', async () => {
    const { d1 } = seeded()
    const { features } = await queryFeatureUsage(d1, { since })
    expect(byId(features, 'tabs')).toMatchObject({ installs: 0, events: 0, everSeen: true })
    expect(byId(features, 'multi-rename')).toMatchObject({ installs: 0, events: 0, everSeen: false, parts: [] })
  })

  it('leaves out firstSeenAt on an open-ended window', async () => {
    const { d1 } = seeded()
    const { features } = await queryFeatureUsage(d1, { since })
    expect(features.every((f) => !('firstSeenAt' in f))).toBe(true)
  })

  it('reads a bounded window as [since, until): its first instant in, its end instant out', async () => {
    const { d1, sqlite } = seeded()
    const until = new Date('2026-11-03T00:00:00.000Z')
    event(sqlite, 'anal_a', 'image_search_used', '2026-11-01T00:00:00.000Z')
    event(sqlite, 'anal_b', 'image_search_used', '2026-11-03T00:00:00.000Z')
    beat(sqlite, 'anal_late', '2026-11-03 00:00:00')
    const { activeInstalls, features } = await queryFeatureUsage(d1, { since, until })
    expect(activeInstalls).toBe(1) // anal_a only: anal_b, anal_c, and anal_late beat on or after `until`
    expect(byId(features, 'image-search')).toMatchObject({ installs: 1, events: 1 })
    expect(byId(features, 'search')).toMatchObject({ installs: 0, events: 0 })
  })

  it('stamps each feature with its earliest stored event on a bounded window, null when never stored', async () => {
    const { d1 } = seeded()
    const until = new Date('2026-11-05T00:00:00.000Z')
    const { features } = await queryFeatureUsage(d1, { since, until })
    expect(byId(features, 'search').firstSeenAt).toBe('2026-10-20T11:00:00.000Z')
    expect(byId(features, 'archives').firstSeenAt).toBe('2026-11-02T09:01:00.000Z')
    expect(byId(features, 'multi-rename').firstSeenAt).toBeNull()
  })

  it('returns every definition, in mapping order, with its sources spelled out', async () => {
    const { d1 } = seeded()
    const { features } = await queryFeatureUsage(d1, { since })
    expect(features.map((f) => f.id)).toEqual(featureUsageDefinitions.map((d) => d.id))
    expect(byId(features, 'archives').sources).toEqual([
      'pane_navigated[volume_kind=archive]',
      'archive_edit_completed',
    ])
  })
})

describe('the mapping', () => {
  const featureStatus = JSON.parse(
    readFileSync(new URL('../../../../feature-status.json', import.meta.url), 'utf8'),
  ) as {
    features: Array<{ id: string; name: string }>
  }

  it('names only real feature-status.json features, by their own names', () => {
    const known = new Map(featureStatus.features.map((f) => [f.id, f.name]))
    for (const def of featureUsageDefinitions) expect([def.id, known.get(def.id)]).toEqual([def.id, def.name])
  })

  it('lists each feature once', () => {
    const ids = featureUsageDefinitions.map((d) => d.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('never lets one stored event match two features (the CASE takes the first arm)', () => {
    // One claim per (event, value); `null` is "every row of this event".
    const claims = featureUsageDefinitions.flatMap((def) =>
      def.matches.flatMap((m) => (m.values ?? [null]).map((value) => ({ id: def.id, event: m.event, value }))),
    )
    const clashes = claims.flatMap((a, i) =>
      claims
        .slice(i + 1)
        .filter((b) => a.event === b.event && (a.value === null || b.value === null || a.value === b.value))
        .map((b) => `${a.event}: ${a.id} and ${b.id}`),
    )
    expect(clashes).toEqual([])
  })

  it('refuses a token that could break out of the SQL', () => {
    const bad = [{ id: 'x', name: 'X', matches: [{ event: "search_used' OR 1=1 --" }] }]
    expect(() => featureUsageSql(bad)).toThrow(/unsafe SQL token/)
    expect(() => seenEventsSql(bad)).toThrow(/unsafe SQL token/)
  })
})

describe('GET /admin/feature-usage', () => {
  const bindings = (d1: D1Database) => ({ TELEMETRY_DB: d1, ADMIN_API_TOKEN: 'test-admin-token-secret' })
  const authHeaders = { Authorization: 'Bearer test-admin-token-secret' }

  it('requires the admin token', async () => {
    const res = await app.request('/admin/feature-usage', {}, bindings(createSqliteD1().d1))
    expect(res.status).toBe(401)
  })

  it('rejects a range it has no window for', async () => {
    const res = await app.request(
      '/admin/feature-usage?range=all',
      { headers: authHeaders },
      bindings(createSqliteD1().d1),
    )
    expect(res.status).toBe(400)
  })

  it('answers the range, its start, the denominator, and one row per feature', async () => {
    const res = await app.request(
      '/admin/feature-usage?range=7d',
      { headers: authHeaders },
      bindings(createSqliteD1().d1),
    )
    expect(res.status).toBe(200)
    const body: FeatureUsageResponse = await res.json()
    expect(body.range).toBe('7d')
    expect(Date.now() - Date.parse(body.since)).toBeGreaterThan(7 * 86_400_000 - 60_000)
    expect(body.activeInstalls).toBe(0)
    expect(body.features).toHaveLength(featureUsageDefinitions.length)
    expect('until' in body).toBe(false)
    expect(body.features.every((f) => !('firstSeenAt' in f))).toBe(true)
  })

  it('rejects an end that is malformed, not a real day, or after today', async () => {
    const tomorrow = new Date(Date.now() + 86_400_000).toISOString().slice(0, 10)
    for (const end of ['2026-11', '2026-02-30', 'yesterday', tomorrow]) {
      const res = await app.request(
        `/admin/feature-usage?range=7d&end=${end}`,
        { headers: authHeaders },
        bindings(createSqliteD1().d1),
      )
      expect(res.status).toBe(400)
    }
  })

  it('answers the complete days ending `end`, with `until` and each feature’s firstSeenAt', async () => {
    const { d1 } = seeded()
    const res = await app.request(
      '/admin/feature-usage?range=7d&end=2026-11-03',
      { headers: authHeaders },
      bindings(d1),
    )
    expect(res.status).toBe(200)
    const body: FeatureUsageResponse = await res.json()
    expect(body.since).toBe('2026-10-28T00:00:00.000Z')
    expect(body.until).toBe('2026-11-04T00:00:00.000Z')
    expect(body.activeInstalls).toBe(2)
    expect(byId(body.features, 'archives')).toMatchObject({ installs: 2, firstSeenAt: '2026-11-02T09:01:00.000Z' })
    expect(byId(body.features, 'multi-rename').firstSeenAt).toBeNull()
  })

  it('accepts today as the end', async () => {
    const todayUtc = new Date().toISOString().slice(0, 10)
    const res = await app.request(
      `/admin/feature-usage?range=7d&end=${todayUtc}`,
      { headers: authHeaders },
      bindings(createSqliteD1().d1),
    )
    expect(res.status).toBe(200)
  })
})
