/**
 * The dialog's state and its review registry: one `GroupReview` per pending group, keyed by
 * group id, and where its rows come from (`rowSource`).
 *
 * The bottom of the module stack, so the trigger (opening, paging, selection, decisions) and the
 * live-facts read (`suggested-ops-live-facts.svelte.ts`) both read the same reviews without
 * importing each other (`import/no-cycle`).
 */

import { SvelteMap, SvelteSet } from 'svelte/reactivity'
import type {
  LiveFileState,
  SuggestedOpLiveFactsAnswer,
  SuggestedOpPage,
  SuggestedGroupView,
  SuggestedOpView,
  SuggestedSweepView,
} from '#lib/tauri-commands/index.js'
import type { DecisionNotice, GroupRefusal } from './suggested-ops-refusal'
import { type OpSelection, storedSelection } from './suggested-ops-selection.svelte'

/** Rows each group's list shows before the user asks for more. */
export const INITIAL_ROWS = 10

/** The rows currently held for one group. */
export interface OpWindow {
  /** Index of `ops[0]` within the whole group. */
  offset: number
  ops: SuggestedOpView[]
  /** Every op row the group has, from `COUNT(*)`. The list sizes itself from this. */
  total: number
}

/** What Cmdr found on disk for one op just now, or that it couldn't tell. */
export type LiveFact = LiveFileState

/** The op counts a group header carries, which the approvable count starts from. */
export type GroupCounts = Pick<SuggestedGroupView, 'liveOpCount' | 'totalOpCount' | 'retiredOpCount'>

/** Everything the dialog holds about one group while the user reads it. */
export interface GroupReview {
  window: OpWindow | null
  windowLoading: boolean
  /** True when the group's last page read threw. The list says so and offers a retry, rather
   *  than leaving its rows on a loading placeholder nothing will ever fill. */
  windowError: boolean
  /** How many rows the list shows: `INITIAL_ROWS`, grown by "Show more" or "Show all". */
  shown: number
  /** Which ops are ticked. Survives scrolling: it is keyed by op id, not by row position. */
  selection: OpSelection
  /** The group `revision` whose rows are on screen. Approve sends it, so the store refuses to
   *  run a version the user hasn't read. Set when the review first reads, and on a reload. */
  seenRevision: number | null
  /** The group's counts at `seenRevision`, set with it. While the group changed under the review
   *  the rows on screen still carry that revision's statuses, so the count reads from these: the
   *  fresh ones already leave out ops another suggestion took over, which the re-check then
   *  calls gone, and counting them out twice read 0 of what's still offered (#1524). */
  seenCounts: GroupCounts | null
  /** Set when the agent amended this group, or a re-read found its op count moved. */
  changedUnderReview: boolean
  /** What the disk says now, per op id, for the rows that have been on screen. */
  liveFacts: SvelteMap<number, LiveFact>
  /** The ops whose file the disk says is gone, by op id. They can't run, so they read unticked
   *  and count for nothing, and approve sends them turned off. Holds the op itself, since its
   *  window may have moved on by the time the user approves. */
  missing: SvelteMap<number, SuggestedOpView>
  /** Ops whose NEW name is already taken where the file lands (#774), by op id. */
  takenNames: SvelteSet<number>
  /** Set when the group's source volume can't be asked at all, so its rows stay on the
   *  snapshot and the dialog says why once, above the list. */
  sourceUnreachable: 'notConnected' | 'gone' | null
  /** The rows the list last drew, so a page that lands late still gets its live facts. */
  range: { start: number; end: number }
}

interface SuggestedOpsState {
  open: boolean
  /** True while the sweep list is loading (the dialog shows a spinner). */
  loading: boolean
  /** True when the read threw: the dialog says so rather than showing an empty list, which
   *  would read as "the agent has suggested nothing". */
  loadError: boolean
  sweeps: SuggestedSweepView[]
  /** One review per pending group, keyed by group id. */
  reviews: SvelteMap<number, GroupReview>
  /** The group whose decision is in flight, so its buttons can disable without freezing the
   *  rest of the dialog. */
  busyGroupId: number | null
  /** Why the last decision didn't happen, or `null`. Set AFTER the re-read, so it speaks about
   *  the list the user is now looking at, and cleared when the next decision starts. */
  decisionNotice: DecisionNotice | null
  /** Groups whose last approval refused to start, keyed by group id. Each one is pending again
   *  and keeps its reason under it until the user answers it again or closes the dialog. */
  refusals: SvelteMap<number, GroupRefusal>
  /** Where op pages and their live facts come from: `null` reads the suggestion store. The
   *  dialog gallery seeds its own, so a fixture group bigger than the rows it seeded still pages
   *  as you scroll (the store has no such group and would answer a total of 0, emptying the list). */
  rowSource: SuggestedOpRowSource | null
}

/** One read a `SuggestedOpRowSource` answers: rows `offset` to `offset + limit` of a group. */
export interface SuggestedOpRowAsk {
  groupId: number
  offset: number
  limit: number
}

/** A stand-in for the store's two row reads, with the same answers (`suggestedOpsState.rowSource`). */
export interface SuggestedOpRowSource {
  page: (ask: SuggestedOpRowAsk) => Promise<SuggestedOpPage>
  liveFacts: (ask: SuggestedOpRowAsk) => Promise<SuggestedOpLiveFactsAnswer>
}

export const suggestedOpsState = $state<SuggestedOpsState>({
  open: false,
  loading: false,
  loadError: false,
  sweeps: [],
  reviews: new SvelteMap<number, GroupReview>(),
  busyGroupId: null,
  decisionNotice: null,
  refusals: new SvelteMap<number, GroupRefusal>(),
  rowSource: null,
})

/** A fresh review, reactive field by field. */
export function newGroupReview(): GroupReview {
  const review = $state<GroupReview>({
    window: null,
    windowLoading: false,
    windowError: false,
    shown: INITIAL_ROWS,
    selection: storedSelection(),
    seenRevision: null,
    seenCounts: null,
    changedUnderReview: false,
    liveFacts: new SvelteMap<number, LiveFact>(),
    missing: new SvelteMap<number, SuggestedOpView>(),
    takenNames: new SvelteSet<number>(),
    sourceUnreachable: null,
    range: { start: 0, end: INITIAL_ROWS },
  })
  return review
}

/** The review for a group, or `undefined` once it's gone. */
export function reviewOf(groupId: number): GroupReview | undefined {
  return suggestedOpsState.reviews.get(groupId)
}
