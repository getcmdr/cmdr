/**
 * Feedback replies: the cron mirrors the board's "owes a reply" onto `feedback.answered_at`, which
 * the dashboard's "Awaiting reply" count reads.
 *
 * David replies from his own mailbox, which the Worker never sees. What he does touch is the card:
 * he closes it with a closing note, or takes `needs-reply` off (`docs/tooling/feedback-and-error-digest.md`
 * § Replying to a reporter). So the board is the truth, and this job copies it.
 *
 * ## The rule
 *
 * A feedback row with a reply-to address and a card (`issue_number > 0`) is awaiting a reply exactly
 * while its card is OPEN and carries `needs-reply`. Otherwise it's answered:
 *
 * - **Answered**: `answered_at` is stamped with the earlier of the card's standing close and its
 *   standing `needs-reply` removal (from the issue events), or the tick's time when the events say
 *   neither (a deleted card, or a label that was never there).
 * - **Back to awaiting**: a card that is open with `needs-reply` again (the person wrote back, or an
 *   amendment brought an address) clears `answered_at`. A reopen alone, without the label, doesn't.
 * - **No card**: rows from before the outbox (`issue_number = 0`) and debug rows (never carded) are
 *   left alone; nothing on the board names them.
 *
 * ## Budget
 *
 * One listing of open `needs-reply` cards per tick (a few pages at most, throws rather than read a
 * partial list), plus one events read per row that just turned answered, at most
 * {@link ANSWERS_PER_TICK}; the rest wait a tick.
 */

import { listOpenCardNumbers, readCardCloseAndUnlabelTimes, resolveIssueTarget } from './github-issues'
import { NEEDS_REPLY_LABEL } from './report-cards'
import type { Bindings } from './types'

/** Pages of 100 open `needs-reply` cards the listing may read. */
const OPEN_CARD_PAGES = 5

/** Rows stamped answered per tick, each costing one events read; the rest wait a tick. */
export const ANSWERS_PER_TICK = 25

interface CardedFeedbackRow {
  id: number
  issue_number: number
  answered_at: string | null
}

/** Every tick: copy the board's "owes a reply" onto `feedback.answered_at`. A no-op unless the reports repo is configured. */
export async function syncFeedbackReplies(env: Bindings, now = new Date()): Promise<void> {
  const issues = resolveIssueTarget(env)
  if (!issues) return

  const db = env.TELEMETRY_DB
  const { results } = await db
    .prepare(
      `SELECT id, issue_number, answered_at FROM feedback
         WHERE issue_number > 0 AND email IS NOT NULL AND TRIM(email) != ''
         ORDER BY id`,
    )
    .all<CardedFeedbackRow>()
  if (results.length === 0) return

  const awaiting = await listOpenCardNumbers(issues, NEEDS_REPLY_LABEL, OPEN_CARD_PAGES)

  for (const row of results) {
    if (row.answered_at !== null && awaiting.has(row.issue_number)) {
      await db.prepare(`UPDATE feedback SET answered_at = NULL WHERE id = ?`).bind(row.id).run()
      console.log(`Feedback replies: #${String(row.id)} awaits a reply again (card #${String(row.issue_number)})`)
    }
  }

  const answered = results
    .filter((row) => row.answered_at === null && !awaiting.has(row.issue_number))
    .slice(0, ANSWERS_PER_TICK)
  for (const row of answered) {
    const times = await readCardCloseAndUnlabelTimes(issues, row.issue_number, NEEDS_REPLY_LABEL)
    const at = earliest(times?.closedAt, times?.labelRemovedAt) ?? now.toISOString()
    await db
      .prepare(`UPDATE feedback SET answered_at = datetime(?1) WHERE id = ?2 AND answered_at IS NULL`)
      .bind(at, row.id)
      .run()
    console.log(`Feedback replies: #${String(row.id)} answered at ${at} (card #${String(row.issue_number)})`)
  }
}

/** The earlier of two ISO 8601 UTC times (same format, so they compare as strings), or null for neither. */
function earliest(a: string | null | undefined, b: string | null | undefined): string | null {
  if (a && b) return a < b ? a : b
  return a ?? b ?? null
}
