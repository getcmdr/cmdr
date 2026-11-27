/**
 * Reactive state and the open/close seam for the AI suggestions dialog.
 *
 * The dialog's job is to lay a suggestion out so the user can decide, which shapes three
 * things here more than any convenience would:
 *
 * - **Every group shows its files right away, and each loads a WINDOW at a time.** A group of
 *   60,000 is legitimate, so nothing loads a group to show it: each group's list knows its
 *   `total` from a `COUNT(*)` and fetches the rows its viewport actually reaches. The list starts
 *   at `INITIAL_ROWS` and grows only when the user asks.
 * - **Selection holds only what the user flipped by hand.** A review opens on the STORED ticks
 *   (an earlier "no" stays off) and records flips against them; select all / none reset to an
 *   empty set over all-or-nothing. Either way the wire stays small at any group size: the user
 *   ticks or unticks a few rows by hand, never 60,000.
 * - **A group that changed under an open review is ANNOUNCED, never swapped.** The rows the
 *   user is reading stay put and a notice offers the reload, because re-ordering a list
 *   somebody is halfway through deciding on is how a wrong row gets approved.
 *
 * This module opens, pages, selects, and decides. The state and its reviews live in
 * `suggested-ops-reviews.svelte.ts`, the disk re-checks in `suggested-ops-live-facts.svelte.ts`,
 * and what each store answer means in `suggested-ops-refusal.ts`.
 */

import type { UnlistenFn } from '@tauri-apps/api/event'
import { tick } from 'svelte'
import { SvelteSet } from 'svelte/reactivity'
import { getAppLogger } from '#lib/logging/logger.js'
import {
  approveSuggestedGroup,
  approveSuggestedServer,
  listSuggestedOps,
  onSuggestionsChanged,
  onWriteSettled,
  pageSuggestedOps,
  rejectSuggestedGroup,
  type SuggestedGroupView,
  type SuggestedOpPage,
  type SuggestedOpView,
} from '#lib/tauri-commands/index.js'
import { endFactsRound, ensureLiveFacts, recheckLiveFacts } from './suggested-ops-live-facts.svelte'
import { decisionNoticeFor, isRefusalToStart, isServerRefusal } from './suggested-ops-refusal'
import {
  INITIAL_ROWS,
  type GroupCounts,
  type GroupReview,
  type OpWindow,
  type SuggestedOpRowAsk,
  newGroupReview,
  reviewOf,
  suggestedOpsState,
} from './suggested-ops-reviews.svelte'
import { flippedOn, isRetiredOp, selectionWire, storedSelection } from './suggested-ops-selection.svelte'

// The registry the dialog, its gallery, and their tests reach through this module.
export {
  INITIAL_ROWS,
  newGroupReview,
  reviewOf,
  suggestedOpsState,
  type GroupReview,
  type LiveFact,
  type SuggestedOpRowSource,
} from './suggested-ops-reviews.svelte'

const log = getAppLogger('suggestedOps')

/** How many op rows one fetch pulls. Comfortably more than a viewport, so ordinary scrolling
 *  reads from memory and only a jump costs a round trip. */
export const OP_WINDOW_SIZE = 200

/** How many more rows "Show more" adds. */
export const SHOW_MORE_STEP = 20

function readPage(ask: SuggestedOpRowAsk): Promise<SuggestedOpPage> {
  const source = suggestedOpsState.rowSource
  return source ? source.page(ask) : pageSuggestedOps(ask.groupId, ask.offset, ask.limit)
}

/** Every group still waiting, flattened out of its sweep. */
export function pendingGroups(): SuggestedGroupView[] {
  return suggestedOpsState.sweeps.flatMap((sweep) => sweep.groups)
}

/** A header's counts, copied: the review keeps them past the next re-read. */
function countsOf(group: SuggestedGroupView): GroupCounts {
  return { liveOpCount: group.liveOpCount, totalOpCount: group.totalOpCount, retiredOpCount: group.retiredOpCount }
}

function groupById(groupId: number): SuggestedGroupView | undefined {
  return pendingGroups().find((group) => group.groupId === groupId)
}

/** Every op row the group has: the page's `COUNT(*)` once one landed, the header's until then. */
export function totalRows(group: SuggestedGroupView): number {
  return reviewOf(group.groupId)?.window?.total ?? group.totalOpCount
}

/** How many rows the group's list draws right now. */
export function shownRows(group: SuggestedGroupView): number {
  const review = reviewOf(group.groupId)
  return Math.min(review?.shown ?? INITIAL_ROWS, totalRows(group))
}

/** "Show more": `SHOW_MORE_STEP` more rows, never past the end. */
export function showMoreRows(groupId: number): void {
  const review = reviewOf(groupId)
  if (review) review.shown += SHOW_MORE_STEP
}

/** "Show all": the whole group, still drawn a window at a time. */
export function showAllRows(groupId: number): void {
  const review = reviewOf(groupId)
  const group = groupById(groupId)
  if (review && group) review.shown = totalRows(group)
}

/** How many ops of a group would actually run. Derived from the COUNTs and the hand-flipped
 *  ids, so it's right even for rows never fetched.
 *
 *  `fromStored` starts from the stored live set (`liveOpCount`, which leaves out ops an earlier
 *  review excluded); `allExcept` counts down from EVERY op row (`totalOpCount`), since the
 *  backend starts that mode from everything the group offers. */
export function approvableCount(groupId: number): number {
  const group = groupById(groupId)
  const review = reviewOf(groupId)
  const selection = review?.selection
  if (!group) return 0
  if (!review || !selection) return group.liveOpCount
  const counts = countsOnScreen(group, review)
  // A file the disk says is gone can't run, so it counts for nothing even where the flips tick it.
  let gone = 0
  for (const op of review.missing.values()) if (flippedOn(selection, op)) gone++
  switch (selection.mode) {
    case 'fromStored':
      return Math.max(0, counts.liveOpCount - selection.exclude.size + selection.include.size - gone)
    case 'only':
      return Math.max(0, selection.ids.size - gone)
    case 'allExcept':
      // A row another suggestion took over never runs, so "all" starts from the rest.
      return Math.max(0, counts.totalOpCount - counts.retiredOpCount - selection.ids.size - gone)
  }
}

/** The counts that describe the rows on screen: the fresh ones, except while the group changed
 *  under the review, whose rows still carry the statuses of the revision the user read. */
function countsOnScreen(group: SuggestedGroupView, review: GroupReview | undefined): GroupCounts {
  return review?.changedUnderReview && review.seenCounts ? review.seenCounts : group
}

/** Whether Approve may run: not while the group changed under the review, since the version
 *  on screen isn't the one that would run until the user reads the new one. */
export function canApprove(groupId: number): boolean {
  if (reviewOf(groupId)?.changedUnderReview === true) return false
  // A server card has no ops to count: the card itself is what the click saves.
  return Boolean(groupById(groupId)?.server) || approvableCount(groupId) > 0
}

/** Whether one op is ticked. Under the stored base, an op an earlier review excluded starts off,
 *  and one whose file is gone is never ticked: it can't run. */
export function isOpSelected(groupId: number, op: SuggestedOpView): boolean {
  const review = reviewOf(groupId)
  if (!review) return op.status === 'pending'
  if (review.missing.has(op.opId)) return false
  return flippedOn(review.selection, op)
}

/** Whether the disk says this op's file is gone, so its row can't be ticked at all. */
export function isOpMissing(groupId: number, op: SuggestedOpView): boolean {
  return reviewOf(groupId)?.missing.has(op.opId) === true
}

/** Whether this op's NEW name is already taken where its file lands (#774). */
export function isNewNameTaken(groupId: number, op: SuggestedOpView): boolean {
  return reviewOf(groupId)?.takenNames.has(op.opId) === true
}

/** The header checkbox: every op ticked, none, or a mix. */
export function selectionState(groupId: number): 'all' | 'none' | 'some' {
  const group = groupById(groupId)
  const count = approvableCount(groupId)
  if (count === 0) return 'none'
  // "All" is every op still there and still offered; a taken-over row that's also gone counts once.
  const review = reviewOf(groupId)
  let gone = 0
  for (const op of review?.missing.values() ?? []) if (!isRetiredOp(op)) gone++
  const counts = group ? countsOnScreen(group, review) : undefined
  if (counts && count >= counts.totalOpCount - counts.retiredOpCount - gone) return 'all'
  return 'some'
}

/** The header checkbox's click: a full list unticks everything, anything else ticks everything. */
export function toggleAllOps(groupId: number): void {
  const review = reviewOf(groupId)
  if (!review) return
  review.selection = {
    mode: selectionState(groupId) === 'all' ? 'only' : 'allExcept',
    ids: new SvelteSet<number>(),
  }
}

/** Live only while the dialog is open: the badge owns the session-long subscription. */
let unlistenChanges: UnlistenFn | null = null
/** Live only while the dialog is open: any write that settles may have moved a drawn file. */
let unlistenSettles: UnlistenFn | null = null

/**
 * Closes the dialog. The last review's state stays put until the next open drops it:
 * clearing it here, in the tick that unmounts the dialog, pulls each row's op out from
 * under its cells mid-teardown (Ark's checkbox reads `checked` on the way out), and the
 * throw takes the whole main window down to its crash panel.
 */
export function closeSuggestedOps(): void {
  suggestedOpsState.open = false
  unlistenChanges?.()
  unlistenChanges = null
  unlistenSettles?.()
  unlistenSettles = null
}

/** Every review, refusal, and notice the last open left behind. Only while nothing renders them. */
function forgetLastReview(): void {
  suggestedOpsState.decisionNotice = null
  suggestedOpsState.reviews.clear()
  endFactsRound()
  suggestedOpsState.refusals.clear()
}

/**
 * Opens the dialog and loads what's waiting. Idempotent, so a menu, palette, and shortcut
 * triple-fire opens it once. Always opens, even on a read failure: a dead menu item is worse
 * than a dialog that says it couldn't read.
 */
export async function openSuggestedOps(): Promise<void> {
  if (suggestedOpsState.open) return
  // Before `open` flips, so the dialog mounts over a clean slate and never sees the old one.
  forgetLastReview()
  suggestedOpsState.open = true
  await subscribeWhileOpen()
  await refreshSuggestions()
}

/**
 * Listen for changes while the dialog is up.
 *
 * The interesting case is `amended` on a group the user is reading. A count comparison alone
 * would miss an amendment that swapped a path but kept the op count, and `groupId` alone can't
 * tell an amendment from the user's own approval: both carry the same id, and only one of them
 * means "the thing you are reading moved".
 */
async function subscribeWhileOpen(): Promise<void> {
  if (!unlistenChanges) {
    try {
      unlistenChanges = await listenForChanges()
    } catch (e) {
      // The dialog opens either way. Losing the subscription costs the live notice, not the
      // review: every refresh still compares each group's op count, so a change that alters it
      // is still announced.
      log.warn("Couldn't subscribe to suggestion changes: {error}", { error: e })
    }
  }
  if (!unlistenSettles) {
    try {
      // An approved group runs in the queue AFTER its claim's announcement, so only the write's
      // settle says its files really moved. Any write counts: a pane rename moves a file too.
      unlistenSettles = await onWriteSettled(() => void recheckLiveFacts())
    } catch (e) {
      // Losing it costs freshness between decisions; every refresh still re-checks.
      log.warn("Couldn't subscribe to write settles: {error}", { error: e })
    }
  }
}

function listenForChanges(): Promise<UnlistenFn> {
  return onSuggestionsChanged((payload) => {
    if (payload.reason === 'amended' && payload.groupId !== null) {
      const review = reviewOf(payload.groupId)
      // The rows on screen stay exactly where they are; the notice offers the reload.
      if (review) review.changedUnderReview = true
    }
    void refreshSuggestions()
  })
}

/**
 * Re-read the waiting sweeps, then give every new group its review and first page.
 *
 * A group already under review keeps its rows: when its revision moved past the one on screen,
 * it's announced, never swapped. A group that's gone (answered here or elsewhere, or withdrawn)
 * drops its review, but only after the flush that takes its rows off screen: dropping it in the
 * same tick pulls each row's op out from under its cells mid-teardown (Ark's checkbox reads
 * `checked` on the way out), and the throw takes the main window down to its crash panel.
 */
export async function refreshSuggestions(): Promise<void> {
  suggestedOpsState.loading = true
  suggestedOpsState.loadError = false
  const fresh: number[] = []
  const held: number[] = []
  const gone: number[] = []
  try {
    suggestedOpsState.sweeps = await listSuggestedOps()
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- transient local for one pass; nothing renders from it
    const live = new Set<number>()
    for (const group of pendingGroups()) {
      live.add(group.groupId)
      const review = reviewOf(group.groupId)
      if (!review) {
        const created = newGroupReview()
        created.seenRevision = group.revision
        created.seenCounts = countsOf(group)
        suggestedOpsState.reviews.set(group.groupId, created)
        fresh.push(group.groupId)
      } else {
        held.push(group.groupId)
        if (review.seenRevision !== null && review.seenRevision !== group.revision) {
          // The group moved since the rows on screen were read: announce it, never swap.
          review.changedUnderReview = true
        }
      }
    }
    for (const groupId of suggestedOpsState.reviews.keys()) {
      if (!live.has(groupId)) gone.push(groupId)
    }
  } catch (e) {
    suggestedOpsState.loadError = true
    log.warn("Couldn't read the waiting suggestions: {error}", { error: e })
  } finally {
    suggestedOpsState.loading = false
  }
  if (gone.length > 0) {
    await tick()
    // A re-read that landed during the tick may have brought one back: it keeps its review.
    for (const groupId of gone) if (!groupById(groupId)) dropReview(groupId)
  }
  // Every list shows right away, so each new group reads its first page now, side by side. A
  // group already on screen re-checks its drawn rows: whatever moved the pending set (an
  // approval, a refusal that gave a group back) may have moved its files too (#1524).
  await Promise.all([
    ...fresh.map((groupId) => ensureOpWindow(groupId, 0)),
    recheckLiveFacts(held.filter((groupId) => reviewOf(groupId) !== undefined)),
  ])
}

function dropReview(groupId: number): void {
  suggestedOpsState.reviews.delete(groupId)
  endFactsRound(groupId)
}

/**
 * "Show the new version" on a group the agent changed: read its rows again from the top.
 *
 * The selection survives on purpose. An op the user turned off stays off if the new version
 * still has it; resetting it would quietly turn their "no" back into a "yes".
 */
export async function reloadGroup(groupId: number): Promise<void> {
  const review = reviewOf(groupId)
  if (!review) return
  review.window = null
  review.liveFacts.clear()
  review.missing.clear()
  review.takenNames.clear()
  review.sourceUnreachable = null
  endFactsRound(groupId)
  await refreshSuggestions()
  // The rows about to load are the new version: that's the one the user now reads.
  const reread = groupById(groupId)
  review.seenRevision = reread?.revision ?? review.seenRevision
  if (reread) review.seenCounts = countsOf(reread)
  review.changedUnderReview = false
  await ensureOpWindow(groupId, 0)
}

/**
 * The list drew rows `start` to `end`: make sure their page is held, then check them on disk.
 *
 * The list calls this as it scrolls. A request for rows already held is a no-op, so ordinary
 * scrolling inside a window costs nothing.
 */
export async function showRange(groupId: number, start: number, end: number): Promise<void> {
  const review = reviewOf(groupId)
  if (!review) return
  review.range = { start, end }
  await ensureOpWindow(groupId, start, end)
  await ensureLiveFacts(groupId)
}

/**
 * Make sure rows `start` to `end` are loaded, fetching a window when any of them isn't.
 *
 * The WHOLE range, not its first row alone: checking only `start` left the bottom of the drawn
 * range on blank placeholders whenever it ran past the held window's end, until the top row
 * crossed too. The list's range changes call this, and so does the retry after a failed read.
 */
export async function ensureOpWindow(groupId: number, start: number, end = start + 1): Promise<void> {
  const review = reviewOf(groupId)
  // A server card has no ops: nothing to page, and with no window, nothing to check on disk.
  if (!review || groupById(groupId)?.server) return
  if (windowHolds(review.window, start, end)) return
  if (review.windowLoading) return

  // Centre the window on what was asked for, so scrolling either way stays inside it: a
  // quarter of the room the range leaves goes above it, the rest below.
  const slack = Math.max(0, OP_WINDOW_SIZE - (end - start))
  const offset = Math.max(0, start - Math.floor(slack / 4))
  review.windowLoading = true
  review.windowError = false
  const rangeAtAsk = { ...review.range }
  try {
    const page = await readPage({ groupId, offset, limit: OP_WINDOW_SIZE })
    // The group may have been answered, or the dialog closed, while this was in flight.
    if (reviewOf(groupId) !== review) return
    review.window = { offset: page.offset, ops: page.ops, total: page.total }
    // A scroll made while this page was in flight was turned away above, and the list won't ask
    // for that range again, so follow it. Only a range that MOVED meanwhile: one past the
    // group's end is never held, and must not loop.
    const now = review.range
    const moved = now.start !== rangeAtAsk.start || now.end !== rangeAtAsk.end
    if (moved && !windowHolds(review.window, now.start, now.end)) {
      review.windowLoading = false
      await ensureOpWindow(groupId, now.start, now.end)
      return
    }
  } catch (e) {
    // The list only asks again when its range moves, so without this the rows would sit on
    // their loading placeholder for good while Approve stays live.
    review.windowError = true
    log.warn("Couldn't read a page of proposed ops: {error}", { error: e })
    return
  } finally {
    review.windowLoading = false
  }
  // A page that lands after the list already drew its rows still owes them their live facts.
  await ensureLiveFacts(groupId)
}

/**
 * Whether the held window covers rows `start` to `end`. A range wider than one page can't be
 * held whole, so only its first `OP_WINDOW_SIZE` rows count; otherwise every scroll would page.
 */
function windowHolds(held: OpWindow | null, start: number, end: number): boolean {
  if (held === null) return false
  const last = Math.max(start, Math.min(end, start + OP_WINDOW_SIZE) - 1)
  return start >= held.offset && last < held.offset + held.ops.length
}

/** The op at a whole-group index, or `null` when its window isn't loaded yet. */
export function opAt(groupId: number, index: number): SuggestedOpView | null {
  const held = reviewOf(groupId)?.window
  if (!held) return null
  return held.ops[index - held.offset] ?? null
}

/** Turn one op off, or back on: flip its membership in whichever set records it against the base. */
export function toggleOp(groupId: number, op: SuggestedOpView): void {
  const review = reviewOf(groupId)
  // A gone file's box is disabled, as is a taken-over row's: there's nothing to tick back on.
  if (!review || review.missing.has(op.opId) || isRetiredOp(op)) return
  const selection = review.selection
  const flips =
    selection.mode !== 'fromStored' ? selection.ids : op.status === 'excluded' ? selection.include : selection.exclude
  if (flips.has(op.opId)) {
    flips.delete(op.opId)
  } else {
    flips.add(op.opId)
  }
}

/**
 * Approve a group: claim it and hand its ops to the queue.
 *
 * Sends the selection in its own mode: the ids the user flipped by hand, never the whole list,
 * and nothing at all while the group changed under the review. On success the dialog gets out
 * of the way once nothing is left, and the queue surface takes over, which is the hand-off the
 * whole feature is built around: an approved op is an ordinary queued op from here on.
 *
 * A refusal is not a failure to hide. Each variant means a different thing to the user, so the
 * dialog re-reads and lets them see the state that actually exists. A refusal to START gives
 * the group back: nothing ran, it's pending again, and its reason sits under it, so the user is
 * where they were and knows why.
 */
export async function approveGroup(groupId: number): Promise<void> {
  if (suggestedOpsState.busyGroupId !== null || !canApprove(groupId)) return
  suggestedOpsState.busyGroupId = groupId
  suggestedOpsState.decisionNotice = null
  suggestedOpsState.refusals.delete(groupId)
  try {
    // The revision the user read, never the one the store holds now: an amendment that landed
    // since refuses as `listChanged` instead of running rows nobody saw.
    const revision = reviewOf(groupId)?.seenRevision ?? groupById(groupId)?.revision ?? 0
    // A server card has no ops to select: it saves through its own command.
    if (groupById(groupId)?.server) {
      await approveServerGroup(groupId, revision)
    } else {
      await approveFileGroup(groupId, revision)
    }
  } catch (e) {
    log.warn("Couldn't approve the group: {error}", { error: e })
    await answerFailedDecision(e)
  } finally {
    suggestedOpsState.busyGroupId = null
  }
}

/** Approve a file group with the user's selection, then re-read. */
async function approveFileGroup(groupId: number, revision: number): Promise<void> {
  const review = reviewOf(groupId)
  const selection = review?.selection ?? storedSelection()
  const wire = selectionWire(selection, review?.missing.values() ?? [])
  // The fallback the review showed, so a trash on a drive with no Trash deletes only when the
  // user approved it against the red Delete button.
  const result = await approveSuggestedGroup(groupId, revision, wire, groupById(groupId)?.runsAs ?? null)
  if (isRefusalToStart(result)) suggestedOpsState.refusals.set(groupId, result)
  if (result.kind !== 'started') {
    log.info('A suggestion group was not approved: {kind}', { kind: result.kind })
  }
  if (result.kind === 'listChanged') markChangedUnderReview(groupId)
  await refreshSuggestions()
  // Nothing waiting means nothing left to decide, so the dialog closes itself rather than
  // sitting there empty over the queue that just took over.
  if (result.kind === 'started' && suggestedOpsState.sweeps.length === 0) closeSuggestedOps()
}

/**
 * Approve a server card: save it, then re-read. A refusal gives the card back pending with its
 * reason under it, so the re-read shows it again; `listChanged` marks it changed under the
 * review, as for a file group.
 */
async function approveServerGroup(groupId: number, revision: number): Promise<void> {
  const result = await approveSuggestedServer(groupId, revision)
  if (result.kind !== 'saved') {
    log.info('A server suggestion was not saved: {kind}', { kind: result.kind })
  }
  if (isServerRefusal(result)) suggestedOpsState.refusals.set(groupId, { kind: 'server', answer: result })
  if (result.kind === 'listChanged') markChangedUnderReview(groupId)
  await refreshSuggestions()
  if (result.kind === 'saved' && suggestedOpsState.sweeps.length === 0) closeSuggestedOps()
}

function markChangedUnderReview(groupId: number): void {
  const review = reviewOf(groupId)
  if (review) review.changedUnderReview = true
}

/**
 * Record the user's "no" for a group, then re-read.
 *
 * A group somebody already answered isn't an error: the dialog reloads so the user sees the
 * state that actually exists rather than being told their click failed.
 */
export async function rejectGroup(groupId: number): Promise<void> {
  if (suggestedOpsState.busyGroupId !== null) return
  suggestedOpsState.busyGroupId = groupId
  suggestedOpsState.decisionNotice = null
  suggestedOpsState.refusals.delete(groupId)
  try {
    const result = await rejectSuggestedGroup(groupId)
    if (result.kind !== 'rejected') {
      log.info('A suggestion group was already answered before this rejection: {kind}', { kind: result.kind })
    }
    await refreshSuggestions()
  } catch (e) {
    log.warn("Couldn't record the rejection: {error}", { error: e })
    await answerFailedDecision(e)
  } finally {
    suggestedOpsState.busyGroupId = null
  }
}

/**
 * After an Approve or Reject that threw: re-read, then say what happened.
 *
 * The re-read comes first because the store is the only thing that knows whether the decision
 * landed, and the group can read as pending while its ops already run.
 */
async function answerFailedDecision(error: unknown): Promise<void> {
  await refreshSuggestions()
  suggestedOpsState.decisionNotice = decisionNoticeFor(error)
}
