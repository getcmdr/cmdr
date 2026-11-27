/**
 * What the disk says now about the rows a review has drawn: size and modified time, that the file
 * is gone, or that its new name is taken (#774).
 *
 * Asked per round: each drawn row once, until something may have moved its file
 * (`recheckLiveFacts`, #1524), and an answer to an older round that lands late goes unread.
 */

import { getAppLogger } from '#lib/logging/logger.js'
import { liveSuggestedOpFacts, type SuggestedOpLiveFactsAnswer } from '#lib/tauri-commands/index.js'
import {
  type GroupReview,
  type OpWindow,
  type SuggestedOpRowAsk,
  reviewOf,
  suggestedOpsState,
} from './suggested-ops-reviews.svelte'

const log = getAppLogger('suggestedOps')

function readLiveFacts(ask: SuggestedOpRowAsk): Promise<SuggestedOpLiveFactsAnswer> {
  const source = suggestedOpsState.rowSource
  return source ? source.liveFacts(ask) : liveSuggestedOpFacts(ask.groupId, ask.offset, ask.limit)
}

/** Op ids whose live facts were already asked for, per group, in the current round. Not state:
 *  nothing renders it, it only keeps a scroll from asking twice. Dropping a group's set starts a
 *  new round (`recheckLiveFacts`), and the set's identity is the round: an answer whose set was
 *  dropped while it was in flight is stale and goes unread. */
// eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping only; nothing renders from it
const factsRequested = new Map<number, Set<number>>()

/** End a group's round (its review is gone, or its rows are about to load again), or every
 *  group's when no id is given, so an answer still in flight for it goes unread. */
export function endFactsRound(groupId?: number): void {
  if (groupId === undefined) factsRequested.clear()
  else factsRequested.delete(groupId)
}

/**
 * Ask the disk again about every drawn row of these groups (default: every group under review),
 * because something may have moved their files since they were last asked.
 *
 * The last answer stays on screen until the new one lands, so a row never blinks back to its
 * snapshot, and an answer to an older round that lands late is dropped (`factsRequested`).
 */
export async function recheckLiveFacts(groupIds: number[] = [...suggestedOpsState.reviews.keys()]): Promise<void> {
  for (const groupId of groupIds) factsRequested.delete(groupId)
  await Promise.all(groupIds.map((groupId) => ensureLiveFacts(groupId)))
}

/**
 * Check the drawn rows on disk: size and modified time now, or that the file is gone.
 *
 * Only the rows on screen, only once each per round: a 60,000-op group shown whole still costs
 * one stat per row the user actually scrolls past. One call covers the span of drawn rows not
 * yet asked about. A held answer is not an excuse to skip a row: `recheckLiveFacts` starts a
 * new round exactly when the held one may be stale.
 *
 * Every answer short of a fact leaves rows on their creation snapshot, which the dialog labels
 * as such. A source that can't be asked at all (not connected, gone) is recorded once on the
 * review and not asked again; the dialog says it once for the group. Anything else is logged,
 * and the rows un-marked so the next scroll asks again.
 */
export async function ensureLiveFacts(groupId: number): Promise<void> {
  const review = reviewOf(groupId)
  const held = review?.window
  if (!review || !held || review.sourceUnreachable !== null) return
  let asked = factsRequested.get(groupId)
  if (!asked) {
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- bookkeeping only, see `factsRequested`
    asked = new Set<number>()
    factsRequested.set(groupId, asked)
  }
  const span = unaskedSpan(review, held, asked)
  if (!span) return
  for (const opId of span.opIds) asked.add(opId)
  try {
    const answer = await readLiveFacts({ groupId, offset: span.offset, limit: span.opIds.length })
    // The group was answered, the dialog closed, or a newer round asked again meanwhile: this
    // answer may predate a move, so the newer round's is the one to show.
    if (reviewOf(groupId) !== review || factsRequested.get(groupId) !== asked) return
    if (takeLiveAnswer(review, held, answer)) return
  } catch (e) {
    log.warn("Couldn't check the suggested files on disk: {error}", { error: e })
  }
  for (const opId of span.opIds) asked.delete(opId)
}

/** The drawn rows nobody has asked about yet, as one contiguous span of the held page. */
function unaskedSpan(
  review: GroupReview,
  held: OpWindow,
  asked: Set<number>,
): { offset: number; opIds: number[] } | null {
  const from = Math.max(review.range.start, held.offset)
  const to = Math.min(review.range.end, held.offset + held.ops.length)
  let first = -1
  let last = -1
  for (let index = from; index < to; index++) {
    const opId = held.ops[index - held.offset].opId
    if (asked.has(opId)) continue
    if (first < 0) first = index
    last = index
  }
  if (first < 0) return null
  return { offset: first, opIds: held.ops.slice(first - held.offset, last + 1 - held.offset).map((op) => op.opId) }
}

/** Record an answer. `false` means it held nothing to keep, so the rows should be asked again. */
function takeLiveAnswer(review: GroupReview, held: OpWindow, answer: SuggestedOpLiveFactsAnswer): boolean {
  if (answer.kind === 'facts') {
    for (const { opId, state, newName } of answer.facts) {
      review.liveFacts.set(opId, state)
      // The op itself, from the page the span was cut from, so approve can still turn it off
      // once the list has scrolled that page away.
      const op = state.kind === 'missing' ? held.ops.find((candidate) => candidate.opId === opId) : undefined
      if (op) review.missing.set(opId, op)
      else review.missing.delete(opId)
      if (newName === 'taken') review.takenNames.add(opId)
      else review.takenNames.delete(opId)
    }
    return true
  }
  switch (answer.reason.type) {
    case 'sourceNotConnected':
      review.sourceUnreachable = 'notConnected'
      return true
    case 'sourceGone':
      review.sourceUnreachable = 'gone'
      return true
    case 'unknownGroup':
    case 'storeNotOpen':
    case 'store':
    case 'unexpected':
      log.warn("Couldn't check the suggested files on disk: {reason}", { reason: answer.reason })
      return false
  }
}
