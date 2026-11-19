/**
 * Feedback replies: the cron job against a REAL SQLite (the production migrations) and a fake GitHub.
 * What's at stake is the dashboard's "Awaiting reply" count: a row that stays awaiting after David
 * answered, or one marked answered while its card still asks for a reply.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createSqliteD1, type SqliteD1 } from './sqlite-d1-test-helper'
import { ANSWERS_PER_TICK, syncFeedbackReplies } from './report-replies'
import type { Bindings } from './types'

const REPO_URL = 'https://api.github.com/repos/getcmdr/cmdr-reports'

function json(value: unknown, status = 200): Promise<Response> {
  return Promise.resolve(
    new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } }),
  )
}

interface FakeEvent {
  event: 'closed' | 'reopened' | 'labeled' | 'unlabeled'
  at: string
  label?: string
}

/** The GitHub this job talks to: the open `needs-reply` listing and each card's events. */
function fakeGitHub(options: {
  /** Open cards that carry `needs-reply`. */
  awaiting: number[]
  events?: Record<number, FakeEvent[]>
  /** Cards GitHub answers 404 for. */
  gone?: number[]
  failListing?: boolean
}) {
  const requests: string[] = []
  vi.spyOn(globalThis, 'fetch').mockImplementation((input: RequestInfo | URL) => {
    const url = input as string
    requests.push(url)
    if (url.startsWith(`${REPO_URL}/issues?`)) {
      if (options.failListing) return json({ message: 'boom' }, 502)
      expect(url).toContain('state=open')
      expect(url).toContain('labels=needs-reply')
      return json(options.awaiting.map((number) => ({ number })))
    }
    const eventsMatch = /\/issues\/(\d+)\/events\?/.exec(url)
    if (eventsMatch) {
      const number = Number(eventsMatch[1])
      if (options.gone?.includes(number)) return json({ message: 'Not Found' }, 404)
      return json(
        (options.events?.[number] ?? []).map((e) => ({
          event: e.event,
          created_at: e.at,
          ...(e.label ? { label: { name: e.label } } : {}),
        })),
      )
    }
    return json({ message: `unexpected ${url}` }, 404)
  })
  return { requests, eventReads: () => requests.filter((url) => url.includes('/events?')) }
}

let db: SqliteD1

function testEnv(overrides: Partial<Bindings> = {}): Bindings {
  return {
    TELEMETRY_DB: db.d1,
    GITHUB_ISSUES_TOKEN: 'github_pat_issues',
    GITHUB_ISSUES_REPO: 'getcmdr/cmdr-reports',
    ...overrides,
  } as Bindings
}

function seed(row: { id: number; issueNumber: number | null; email?: string | null; answeredAt?: string | null }) {
  db.sqlite
    .prepare(
      `INSERT INTO feedback (id, created_at, feedback, email, app_version, os_version, build_mode, issue_number, answered_at)
         VALUES (?, '2026-10-20 10:00:00', 'Love it', ?, '0.52.0', '15.7', 'release', ?, ?)`,
    )
    .run(
      row.id,
      row.email === undefined ? 'test_user_1@example.com' : row.email,
      row.issueNumber,
      row.answeredAt ?? null,
    )
}

function answeredAt(id: number): string | null {
  const row = db.sqlite.prepare(`SELECT answered_at FROM feedback WHERE id = ?`).get(id) as {
    answered_at: string | null
  }
  return row.answered_at
}

const now = new Date('2026-11-19T09:00:00Z')

beforeEach(() => {
  db = createSqliteD1()
  vi.spyOn(console, 'log').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('syncFeedbackReplies', () => {
  it('leaves a row whose card is open and still needs a reply awaiting', async () => {
    seed({ id: 15, issueNumber: 73 })
    const github = fakeGitHub({ awaiting: [73] })

    await syncFeedbackReplies(testEnv(), now)

    expect(answeredAt(15)).toBeNull()
    expect(github.eventReads()).toEqual([])
  })

  it('stamps a closed card answered at its close time', async () => {
    seed({ id: 15, issueNumber: 73 })
    fakeGitHub({ awaiting: [], events: { 73: [{ event: 'closed', at: '2026-10-20T10:05:49Z' }] } })

    await syncFeedbackReplies(testEnv(), now)

    expect(answeredAt(15)).toBe('2026-10-20 10:05:49')
  })

  it('stamps an open card that lost needs-reply answered at the label removal', async () => {
    seed({ id: 16, issueNumber: 76 })
    fakeGitHub({
      awaiting: [],
      events: {
        76: [
          { event: 'labeled', at: '2026-10-21T14:01:30Z', label: 'needs-reply' },
          { event: 'unlabeled', at: '2026-10-21T15:00:00Z', label: 'unprocessed' },
          { event: 'unlabeled', at: '2026-10-23T08:00:00Z', label: 'needs-reply' },
        ],
      },
    })

    await syncFeedbackReplies(testEnv(), now)

    expect(answeredAt(16)).toBe('2026-10-23 08:00:00')
  })

  it('takes the earlier of the close and the label removal', async () => {
    seed({ id: 18, issueNumber: 79 })
    fakeGitHub({
      awaiting: [],
      events: {
        79: [
          { event: 'unlabeled', at: '2026-10-22T20:00:00Z', label: 'needs-reply' },
          { event: 'closed', at: '2026-10-23T08:26:28Z' },
        ],
      },
    })

    await syncFeedbackReplies(testEnv(), now)

    expect(answeredAt(18)).toBe('2026-10-22 20:00:00')
  })

  it('ignores a close that was undone by a reopen, and a removal undone by a re-add', async () => {
    seed({ id: 20, issueNumber: 90 })
    fakeGitHub({
      awaiting: [],
      events: {
        90: [
          { event: 'closed', at: '2026-10-31T00:00:00Z' },
          { event: 'reopened', at: '2026-11-01T00:00:00Z' },
          { event: 'unlabeled', at: '2026-11-02T00:00:00Z', label: 'needs-reply' },
          { event: 'labeled', at: '2026-11-03T00:00:00Z', label: 'needs-reply' },
          { event: 'closed', at: '2026-11-12T10:38:08Z' },
        ],
      },
    })

    await syncFeedbackReplies(testEnv(), now)

    expect(answeredAt(20)).toBe('2026-11-12 10:38:08')
  })

  it("falls back to the tick's time when the card is gone or its events name neither", async () => {
    seed({ id: 21, issueNumber: 91 })
    seed({ id: 22, issueNumber: 92 })
    fakeGitHub({ awaiting: [], gone: [91], events: { 92: [] } })

    await syncFeedbackReplies(testEnv(), now)

    expect(answeredAt(21)).toBe('2026-11-19 09:00:00')
    expect(answeredAt(22)).toBe('2026-11-19 09:00:00')
  })

  it('leaves rows without a card alone: pre-outbox (issue 0), not yet filed, and no reply-to', async () => {
    seed({ id: 8, issueNumber: 0 })
    seed({ id: 30, issueNumber: null })
    seed({ id: 31, issueNumber: 120, email: null })
    seed({ id: 32, issueNumber: 121, email: '  ' })
    const github = fakeGitHub({ awaiting: [] })

    await syncFeedbackReplies(testEnv(), now)

    expect([8, 30, 31, 32].map(answeredAt)).toEqual([null, null, null, null])
    // Nothing to sync means no GitHub calls at all.
    expect(github.requests).toEqual([])
  })

  it('keeps an answered time already stamped, with no events read', async () => {
    seed({ id: 15, issueNumber: 73, answeredAt: '2026-10-20 10:05:49' })
    const github = fakeGitHub({ awaiting: [] })

    await syncFeedbackReplies(testEnv(), now)

    expect(answeredAt(15)).toBe('2026-10-20 10:05:49')
    expect(github.eventReads()).toEqual([])
  })

  it('puts an answered row back to awaiting when its card is open with needs-reply again', async () => {
    seed({ id: 15, issueNumber: 73, answeredAt: '2026-10-20 10:05:49' })
    fakeGitHub({ awaiting: [73] })

    await syncFeedbackReplies(testEnv(), now)

    expect(answeredAt(15)).toBeNull()
  })

  it(`stamps at most ${String(ANSWERS_PER_TICK)} rows a tick, and the rest on the next`, async () => {
    for (let i = 1; i <= ANSWERS_PER_TICK + 2; i++) seed({ id: i, issueNumber: 200 + i })
    const github = fakeGitHub({ awaiting: [] })

    await syncFeedbackReplies(testEnv(), now)
    expect(github.eventReads()).toHaveLength(ANSWERS_PER_TICK)
    expect(answeredAt(ANSWERS_PER_TICK + 1)).toBeNull()

    await syncFeedbackReplies(testEnv(), now)
    expect(answeredAt(ANSWERS_PER_TICK + 2)).not.toBeNull()
  })

  it('throws on a failed listing and changes nothing, so the alarm fires and the next tick retries', async () => {
    seed({ id: 15, issueNumber: 73 })
    fakeGitHub({ awaiting: [], failListing: true })

    await expect(syncFeedbackReplies(testEnv(), now)).rejects.toThrow('listing open needs-reply cards')
    expect(answeredAt(15)).toBeNull()
  })

  it('throws rather than read a cut-off listing as labels removed', async () => {
    seed({ id: 15, issueNumber: 73 })
    vi.spyOn(globalThis, 'fetch').mockImplementation(() =>
      json(Array.from({ length: 100 }, (_, i) => ({ number: 1000 + i }))),
    )

    await expect(syncFeedbackReplies(testEnv(), now)).rejects.toThrow('refusing to read a partial list')
    expect(answeredAt(15)).toBeNull()
  })

  it('is a no-op without the reports repo configured', async () => {
    seed({ id: 15, issueNumber: 73 })
    const github = fakeGitHub({ awaiting: [] })

    await syncFeedbackReplies(testEnv({ GITHUB_ISSUES_TOKEN: undefined }), now)

    expect(github.requests).toEqual([])
    expect(answeredAt(15)).toBeNull()
  })
})
