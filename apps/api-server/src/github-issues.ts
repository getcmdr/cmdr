/**
 * The GitHub side of the private reports board: filing a card, commenting, labelling, listing, and
 * the personal-comment sweep. What goes ON a card is `report-cards.ts`; when it's filed is
 * `report-outbox.ts`.
 *
 * The invariant that lives here, load-bearing for the privacy policy
 * (`apps/website/src/pages/privacy-policy.astro`): **the repo must be private, checked on every
 * write.** {@link fileIssue} probes the repo and refuses to post anything unless GitHub answers
 * `private: true`. It fails CLOSED: a lookup that errors, 404s, or omits the field writes nothing.
 * That's what stops a misconfigured `GITHUB_ISSUES_REPO` (or a repo someone flips to public) from
 * publishing a user's note.
 */

import type { Bindings } from './types'
import { githubFetch } from './github-fetch'
import {
  buildErrorReportIssue,
  personalCommentExpiry,
  type ErrorReportIssueInput,
  type IssueContent,
  type IssueSource,
} from './report-cards'

const GITHUB_API = 'https://api.github.com'

/**
 * GitHub rejects API requests without a User-Agent. Naming the Worker makes a misbehaving
 * integration identifiable in GitHub's own logs rather than anonymous traffic.
 */
const USER_AGENT = 'cmdr-api-server'

/** Where issues are filed. Absent config means the integration is off, never a fallback repo. */
export interface IssueTarget {
  token: string
  owner: string
  repo: string
}

/**
 * Read the target from config. Returns null (integration off) unless BOTH a token and an
 * `owner/name` repo are present, so a half-configured environment posts nothing instead of guessing.
 */
export function resolveIssueTarget(env: Bindings): IssueTarget | null {
  const token = env.GITHUB_ISSUES_TOKEN
  const repo = env.GITHUB_ISSUES_REPO
  if (!token || !repo) return null

  const parts = repo.split('/')
  if (parts.length !== 2) return null
  const [owner, name] = parts
  if (!owner || !name) return null

  return { token, owner, repo: name }
}

/** Narrow a parsed JSON value to something whose keys can be read. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function githubHeaders(token: string): Record<string, string> {
  return {
    authorization: `Bearer ${token}`,
    accept: 'application/vnd.github+json',
    'x-github-api-version': '2022-11-28',
    'user-agent': USER_AGENT,
  }
}

/**
 * Whether GitHub says this repo is private, right now.
 *
 * Fails CLOSED in every uncertain case: a network error, a non-200, or a payload without an explicit
 * boolean `private: true` all answer false. This is the gate that keeps a user's note off a public
 * repo, so "we couldn't tell" has to mean "don't write".
 */
export async function isRepoPrivate(target: IssueTarget): Promise<boolean> {
  try {
    const response = await githubFetch(`${GITHUB_API}/repos/${target.owner}/${target.repo}`, {
      headers: githubHeaders(target.token),
    })
    if (!response.ok) {
      console.error(`GitHub issues: repo probe returned ${String(response.status)}; refusing to write`)
      return false
    }
    const repo: unknown = await response.json()
    // Strictly `true`, never truthy: "private" as a string, or a missing field, must not pass.
    return isRecord(repo) && repo['private'] === true
  } catch (e) {
    console.error('GitHub issues: repo probe failed; refusing to write', e)
    return false
  }
}

/** An issue plus the optional personal half that gets its own, expiring, comment. */
export interface FileIssueInput extends IssueContent {
  personalComment: string | null
}

/**
 * Create one issue, then attach its personal-data comment.
 *
 * Returns the issue number, or null when nothing was created. The privacy probe runs FIRST and a
 * negative answer returns before any write: that ordering is the whole guarantee, so ❌ never reorder
 * these two calls or make the probe conditional.
 *
 * A comment that fails to post still leaves the issue: the technical record is worth keeping, and
 * the note is not lost (it's in the bundle in R2, or the D1 row).
 */
export async function fileIssue(target: IssueTarget, input: FileIssueInput): Promise<number | null> {
  if (!(await isRepoPrivate(target))) {
    console.error(
      `GitHub issues: ${target.owner}/${target.repo} is not private (or could not be verified); nothing was written`,
    )
    return null
  }

  let issueNumber: number
  try {
    const response = await githubFetch(`${GITHUB_API}/repos/${target.owner}/${target.repo}/issues`, {
      method: 'POST',
      headers: { ...githubHeaders(target.token), 'content-type': 'application/json' },
      body: JSON.stringify({ title: input.title, body: input.body, labels: input.labels }),
    })
    if (!response.ok) {
      console.error(`GitHub issues: create failed with ${String(response.status)}`)
      return null
    }
    const created: unknown = await response.json()
    const number = isRecord(created) ? created['number'] : undefined
    if (typeof number !== 'number') {
      console.error('GitHub issues: create returned no issue number')
      return null
    }
    issueNumber = number
  } catch (e) {
    console.error('GitHub issues: create threw', e)
    return null
  }

  if (input.personalComment) {
    try {
      const response = await githubFetch(
        `${GITHUB_API}/repos/${target.owner}/${target.repo}/issues/${String(issueNumber)}/comments`,
        {
          method: 'POST',
          headers: { ...githubHeaders(target.token), 'content-type': 'application/json' },
          body: JSON.stringify({ body: input.personalComment }),
        },
      )
      if (!response.ok) {
        console.error(
          `GitHub issues: personal comment failed with ${String(response.status)} on #${String(issueNumber)}`,
        )
      }
    } catch (e) {
      console.error(`GitHub issues: personal comment threw on #${String(issueNumber)}`, e)
    }
  }

  return issueNumber
}

/**
 * Issues one source may file per UTC day.
 *
 * Real volume is a handful, so this never touches legitimate traffic. It exists because `kind` and
 * `buildMode` come from the client's manifest: a buggy or hostile build costs 20 issues a day and
 * then waits. Nothing is lost when it trips: the outbox row stays pending and a later day files it.
 */
export const DAILY_ISSUE_CAP = 20

/** Day-scoped keys outlive their day, then expire on their own. Mirrors `error-report-intake.ts`. */
const DAY_KEY_TTL_SECONDS = 48 * 60 * 60

/**
 * Take one slot from a source's daily allowance. Racy like the other KV counters here, which at
 * worst shifts the cutoff by an issue or two.
 */
export async function claimIssueSlot(kv: KVNamespace, source: IssueSource, date: string): Promise<boolean> {
  const key = `gh_issue_count:${source}:${date}`
  const count = parseInt((await kv.get(key)) ?? '0', 10) + 1
  await kv.put(key, String(count), { expirationTtl: DAY_KEY_TTL_SECONDS })
  return count <= DAILY_ISSUE_CAP
}

/** Today in UTC, `YYYY-MM-DD`. */
export function todayUtc(): string {
  return new Date().toISOString().slice(0, 10)
}

/**
 * KV key remembering which issue one report got, so an amendment months later can find its card.
 *
 * Its OWN key rather than a field on the `report:{id}` index: that entry is written before the 200
 * and carries the amend credential's hash, so adding to it would mean a read-modify-write against
 * an eventually-consistent store, with the credential as the thing at risk. A separate key is
 * written once and read once, and a miss simply means no comment.
 */
function issueNumberKey(id: string): string {
  return `gh_issue:${id}`
}

/**
 * Remember an issue number for {@link recallIssueNumber}. TTL matches the report index and the R2
 * lifecycle, so the pointer never outlives the report it points at.
 */
export async function rememberIssueNumber(kv: KVNamespace, id: string, issueNumber: number): Promise<void> {
  await kv.put(issueNumberKey(id), String(issueNumber), { expirationTtl: REPORT_RETENTION_SECONDS })
}

/** The issue one report got, or null when it never got one (auto-send, debug build, cap, outage). */
export async function recallIssueNumber(kv: KVNamespace, id: string): Promise<number | null> {
  const raw = await kv.get(issueNumberKey(id))
  if (raw === null) return null
  const parsed = Number(raw)
  return Number.isInteger(parsed) ? parsed : null
}

/** 90 days, the window a report and everything pointing at it share. */
const REPORT_RETENTION_SECONDS = 90 * 24 * 60 * 60

/** What one amendment needs to reach the board, and how to describe its report if it has no card. */
export interface AmendmentOnBoardInput {
  id: string
  /** The built amendment comment, already stamped with the original report's expiry date. */
  comment: string
  /** Labels the comment path adds. A filed card gets its labels from the report facts instead. */
  addLabels?: string[]
  /**
   * The report's technical facts, read only when a card has to be created. Null when they can't be
   * read any more, which means no card: a card that can't say what it is about helps nobody.
   */
  readReportFacts: () => Promise<ErrorReportIssueInput | null>
  /** The report's card, when the caller already knows it. Skips the KV lookup. */
  issueNumber?: number
}

/** What happened to one amendment on the board. Every failure is a `skipped`. */
export type AmendmentOnBoardResult = 'commented' | 'filed' | 'skipped'

/**
 * Put one amendment on the board.
 *
 * Usually the report already has a card and the amendment is a comment on it. The outbox passes
 * that card's number when it knows it; otherwise the KV pointer is asked. When the report has no
 * card at all (it predates the outbox, so it never got one), the amendment IS the card: a person
 * typed a sentence about their own report, and that has to reach the board.
 *
 * Runs the privacy probe again through {@link fileIssue}'s own gate rather than trusting that the
 * repo was private when the issue was filed: months can pass between an upload and its amendment,
 * and the answer is allowed to have changed.
 */
export async function recordAmendmentOnBoard(
  env: Bindings,
  input: AmendmentOnBoardInput,
): Promise<AmendmentOnBoardResult> {
  const target = resolveIssueTarget(env)
  if (!target) return 'skipped'

  const issueNumber = input.issueNumber ?? (await recallIssueNumber(env.ERROR_REPORT_META, input.id))
  if (issueNumber === null) {
    return (await fileCardForAmendment(env, target, input)) ? 'filed' : 'skipped'
  }
  return (await commentOnIssue(target, issueNumber, input.comment, input.addLabels ?? [])) ? 'commented' : 'skipped'
}

/**
 * File the card an amended report never got, carrying the amendment as its personal comment.
 *
 * Spends the same daily allowance as the upload path, for the same reason: `kind` and `buildMode`
 * come from the client's manifest. Debug builds stay out, they are our own E2E traffic.
 */
async function fileCardForAmendment(
  env: Bindings,
  target: IssueTarget,
  input: AmendmentOnBoardInput,
): Promise<boolean> {
  const facts = await input.readReportFacts()
  if (!facts) return false
  if (facts.buildMode === 'debug') return false
  if (!(await claimIssueSlot(env.ERROR_REPORT_META, 'error-report', todayUtc()))) {
    console.error('GitHub issues: daily error-report cap reached; the amendment did not get a card')
    return false
  }

  const issueNumber = await fileIssue(target, {
    ...buildErrorReportIssue({ ...facts, filedFromAmendment: true }),
    personalComment: input.comment,
  })
  if (issueNumber === null) return false

  // Remembered so the NEXT amendment comments on this card instead of filing a second one.
  try {
    await rememberIssueNumber(env.ERROR_REPORT_META, input.id, issueNumber)
  } catch (e) {
    console.error('GitHub issues: remembering the issue number failed; a later amendment will file its own card', e)
  }
  return true
}

/** Post one comment on an existing card, then add any labels. False when nothing was posted. */
async function commentOnIssue(
  target: IssueTarget,
  issueNumber: number,
  body: string,
  addLabels: string[],
): Promise<boolean> {
  if (!(await isRepoPrivate(target))) {
    console.error(`GitHub issues: ${target.owner}/${target.repo} is not private; the amendment was not posted`)
    return false
  }

  try {
    const response = await githubFetch(
      `${GITHUB_API}/repos/${target.owner}/${target.repo}/issues/${String(issueNumber)}/comments`,
      {
        method: 'POST',
        headers: { ...githubHeaders(target.token), 'content-type': 'application/json' },
        body: JSON.stringify({ body }),
      },
    )
    if (!response.ok) {
      console.error(
        `GitHub issues: amendment comment failed with ${String(response.status)} on #${String(issueNumber)}`,
      )
      return false
    }
  } catch (e) {
    console.error(`GitHub issues: amendment comment threw on #${String(issueNumber)}`, e)
    return false
  }

  if (addLabels.length) {
    await addIssueLabels(target, issueNumber, addLabels)
  }
  return true
}

/**
 * Add labels to an issue, leaving the ones already on it alone: GitHub's add endpoint is additive
 * and idempotent, so re-adding one is a no-op rather than an error.
 *
 * Failure is logged and swallowed. The comment it accompanies is already posted, and losing a label
 * must not read to the caller as losing the amendment.
 */
async function addIssueLabels(target: IssueTarget, issueNumber: number, labels: string[]): Promise<void> {
  try {
    const response = await githubFetch(
      `${GITHUB_API}/repos/${target.owner}/${target.repo}/issues/${String(issueNumber)}/labels`,
      {
        method: 'POST',
        headers: { ...githubHeaders(target.token), 'content-type': 'application/json' },
        body: JSON.stringify({ labels }),
      },
    )
    if (!response.ok) {
      console.error(`GitHub issues: labelling #${String(issueNumber)} returned ${String(response.status)}`)
    }
  } catch (e) {
    console.error(`GitHub issues: labelling #${String(issueNumber)} threw`, e)
  }
}

/** One open card, as the morning brief lists it. */
export interface OpenCard {
  number: number
  title: string
  url: string
  /** ISO 8601, GitHub's `created_at`. */
  createdAt: string
}

/**
 * The open cards carrying `label`, newest first, up to 100 (the brief's lists are a glance, not an
 * archive). Throws on a failed listing so the caller can say the board was unreachable rather than
 * report an empty one. A read, so no privacy probe: nothing is written.
 */
export async function listOpenCards(target: IssueTarget, label: string): Promise<OpenCard[]> {
  const url = `${GITHUB_API}/repos/${target.owner}/${target.repo}/issues?state=open&per_page=100&labels=${encodeURIComponent(label)}`
  const response = await githubFetch(url, { headers: githubHeaders(target.token) })
  if (!response.ok) {
    throw new Error(`GitHub issues: listing ${label} cards returned ${String(response.status)}`)
  }
  const payload: unknown = await response.json()
  const items: unknown[] = Array.isArray(payload) ? payload : []
  const cards: OpenCard[] = []
  for (const item of items) {
    if (!isRecord(item) || 'pull_request' in item) continue
    const { number, title, html_url: htmlUrl, created_at: createdAt } = item
    if (typeof number !== 'number' || typeof title !== 'string') continue
    cards.push({
      number,
      title,
      url: typeof htmlUrl === 'string' ? htmlUrl : '',
      createdAt: typeof createdAt === 'string' ? createdAt : '',
    })
  }
  return cards
}

/** The three fields we read off a GitHub comment, once it has been checked to actually have them. */
interface GitHubComment {
  id: number
  body: string
  issueUrl: string
}

/** Narrow one element of GitHub's comment listing, or null when it isn't shaped as we need. */
function asGitHubComment(raw: unknown): GitHubComment | null {
  if (!raw || typeof raw !== 'object') return null
  const record = raw as Record<string, unknown>
  const id = record['id']
  const body = record['body']
  const issueUrl = record['issue_url']
  if (typeof id !== 'number' || typeof body !== 'string') return null
  return { id, body, issueUrl: typeof issueUrl === 'string' ? issueUrl : '' }
}

/**
 * The issue number at the end of a comment's `issue_url`. Zero when it can't be read, which only
 * costs the log line its issue reference: the comment id is what the delete actually needs.
 */
function issueNumberFromUrl(issueUrl: string): number {
  const last = issueUrl.split('/').pop() ?? ''
  const parsed = Number(last)
  return Number.isFinite(parsed) ? parsed : 0
}

/** One comment the retention sweep may delete, with the date it was promised to go. */
export interface ExpiringComment {
  issueNumber: number
  commentId: number
  expiresOn: string
}

/**
 * Every personal-data comment in the repo that is due for deletion on `today` or earlier.
 *
 * Walks issue comments repo-wide (newest issues carry the youngest stamps, but a 730-day feedback
 * stamp can sit behind 90-day error-report ones, so the walk can't stop early on the first
 * unexpired comment).
 */
export async function listExpiredPersonalComments(target: IssueTarget, today: string): Promise<ExpiringComment[]> {
  const due: ExpiringComment[] = []
  const perPage = 100

  for (let page = 1; page <= 20; page++) {
    const url = `${GITHUB_API}/repos/${target.owner}/${target.repo}/issues/comments?per_page=${String(perPage)}&page=${String(page)}`
    const response = await githubFetch(url, { headers: githubHeaders(target.token) })
    if (!response.ok) {
      throw new Error(`GitHub issues: listing comments returned ${String(response.status)}`)
    }
    const payload: unknown = await response.json()
    const comments: unknown[] = Array.isArray(payload) ? payload : []
    if (comments.length === 0) return due

    for (const raw of comments) {
      const comment = asGitHubComment(raw)
      if (!comment) continue
      const expiresOn = personalCommentExpiry(comment.body)
      if (!expiresOn || expiresOn > today) continue
      due.push({ issueNumber: issueNumberFromUrl(comment.issueUrl), commentId: comment.id, expiresOn })
    }

    if (comments.length < perPage) return due
  }

  return due
}

/** Delete one comment. Throws on failure so the cron's alarm path reports it. */
export async function deleteComment(target: IssueTarget, commentId: number): Promise<void> {
  const response = await githubFetch(
    `${GITHUB_API}/repos/${target.owner}/${target.repo}/issues/comments/${String(commentId)}`,
    { method: 'DELETE', headers: githubHeaders(target.token) },
  )
  // 404 means it's already gone, which is the state we wanted.
  if (!response.ok && response.status !== 404) {
    throw new Error(`GitHub issues: deleting comment ${String(commentId)} returned ${String(response.status)}`)
  }
}

/** One comment as the rating job reads it. */
export interface CardComment {
  id: number
  issueNumber: number
  /** The GitHub login that posted it. */
  author: string
  body: string
  /** ISO 8601. */
  createdAt: string
  updatedAt: string
}

/**
 * Every comment in the repo created or edited at or after `since` (ISO 8601), least recently
 * updated first, up to `maxPages` pages of 100. Throws on a failed page, so the caller can tell an
 * unreachable repo from a quiet one. A read, so no privacy probe: nothing is written.
 */
export async function listCommentsUpdatedSince(
  target: IssueTarget,
  since: string,
  maxPages: number,
): Promise<{ comments: CardComment[]; truncated: boolean }> {
  const comments: CardComment[] = []
  const perPage = 100

  for (let page = 1; page <= maxPages; page++) {
    const url = `${GITHUB_API}/repos/${target.owner}/${target.repo}/issues/comments?sort=updated&direction=asc&since=${encodeURIComponent(since)}&per_page=${String(perPage)}&page=${String(page)}`
    const response = await githubFetch(url, { headers: githubHeaders(target.token) })
    if (!response.ok) {
      throw new Error(`GitHub issues: listing comments returned ${String(response.status)}`)
    }
    const payload: unknown = await response.json()
    const raws: unknown[] = Array.isArray(payload) ? payload : []
    for (const raw of raws) {
      if (!isRecord(raw)) continue
      const { id, body, issue_url: issueUrl, user, created_at: createdAt, updated_at: updatedAt } = raw
      const author = isRecord(user) ? user['login'] : undefined
      if (typeof id !== 'number' || typeof body !== 'string' || typeof author !== 'string') continue
      if (typeof issueUrl !== 'string' || typeof createdAt !== 'string' || typeof updatedAt !== 'string') continue
      const issueNumber = issueNumberFromUrl(issueUrl)
      if (issueNumber <= 0) continue
      comments.push({ id, issueNumber, author, body, createdAt, updatedAt })
    }
    if (raws.length < perPage) return { comments, truncated: false }
  }

  return { comments, truncated: true }
}

/**
 * The numbers of every open card carrying `label`, across up to `maxPages` pages of 100. Throws on a
 * failed page AND when the last page allowed is full: a caller treats a card missing from this set
 * as "no longer carries the label", so a silently cut-off listing would read as dozens of labels
 * removed. A read, so no privacy probe: nothing is written.
 */
export async function listOpenCardNumbers(target: IssueTarget, label: string, maxPages: number): Promise<Set<number>> {
  const numbers = new Set<number>()
  const perPage = 100
  for (let page = 1; page <= maxPages; page++) {
    const url = `${GITHUB_API}/repos/${target.owner}/${target.repo}/issues?state=open&labels=${encodeURIComponent(label)}&per_page=${String(perPage)}&page=${String(page)}`
    const response = await githubFetch(url, { headers: githubHeaders(target.token) })
    if (!response.ok) {
      throw new Error(`GitHub issues: listing open ${label} cards returned ${String(response.status)}`)
    }
    const payload: unknown = await response.json()
    const items: unknown[] = Array.isArray(payload) ? payload : []
    for (const item of items) {
      if (!isRecord(item) || 'pull_request' in item) continue
      if (typeof item['number'] === 'number') numbers.add(item['number'])
    }
    if (items.length < perPage) return numbers
  }
  throw new Error(
    `GitHub issues: more than ${String(maxPages * perPage)} open ${label} cards; refusing to read a partial list`,
  )
}

/** Pages of 100 events read per card. A feedback card has a handful; past this, the times may be a little off. */
const EVENT_PAGES = 3

/**
 * When a card was closed and when it lost `label`, from its issue events: the closing that still
 * stands (null when reopened since, or never closed) and the label removal that still stands (null
 * when added back, or never removed). Null for a card GitHub no longer has (404, or 410 for a
 * deleted issue). Throws on any other failure, so the caller retries next tick.
 */
export async function readCardCloseAndUnlabelTimes(
  target: IssueTarget,
  issueNumber: number,
  label: string,
): Promise<{ closedAt: string | null; labelRemovedAt: string | null } | null> {
  const times: CloseAndUnlabelTimes = { closedAt: null, labelRemovedAt: null }
  const perPage = 100
  for (let page = 1; page <= EVENT_PAGES; page++) {
    const url = `${GITHUB_API}/repos/${target.owner}/${target.repo}/issues/${String(issueNumber)}/events?per_page=${String(perPage)}&page=${String(page)}`
    const response = await githubFetch(url, { headers: githubHeaders(target.token) })
    if (response.status === 404 || response.status === 410) return null
    if (!response.ok) {
      throw new Error(`GitHub issues: reading #${String(issueNumber)}'s events returned ${String(response.status)}`)
    }
    const payload: unknown = await response.json()
    const events: unknown[] = Array.isArray(payload) ? payload : []
    // Oldest first, so the last event of each pair decides.
    for (const event of events) foldCloseAndUnlabel(times, event, label)
    if (events.length < perPage) break
  }
  return times
}

interface CloseAndUnlabelTimes {
  closedAt: string | null
  labelRemovedAt: string | null
}

/** Apply one issue event: a close or a label removal sets its time, a reopen or a re-add cancels it. */
function foldCloseAndUnlabel(times: CloseAndUnlabelTimes, event: unknown, label: string): void {
  if (!isRecord(event) || typeof event['created_at'] !== 'string') return
  const at = event['created_at']
  const ofLabel = isRecord(event['label']) && event['label']['name'] === label
  if (event['event'] === 'closed') times.closedAt = at
  else if (event['event'] === 'reopened') times.closedAt = null
  else if (event['event'] === 'unlabeled' && ofLabel) times.labelRemovedAt = at
  else if (event['event'] === 'labeled' && ofLabel) times.labelRemovedAt = null
}
