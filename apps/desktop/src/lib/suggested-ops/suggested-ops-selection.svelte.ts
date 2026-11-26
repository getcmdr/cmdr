/**
 * A group's selection: which of its ops would run, as the user's hand flips over a base, and how
 * that goes over the wire. No store behind it, so the trigger owns every review and this only
 * answers questions about one selection.
 */

import { SvelteSet } from 'svelte/reactivity'
import type { OpSelection as WireOpSelection, SuggestedOpView } from '#lib/tauri-commands/index.js'

/**
 * Which of a group's ops would run, as flips over a base.
 *
 * - `fromStored`: the stored ticks (an op an earlier review turned off arrives `excluded`) with
 *   `exclude` turned off and `include` turned back on. The state a group opens in, so a reopened
 *   review keeps the user's earlier "no"s without loading every op to find them.
 * - `allExcept`: every op but `ids` (after the header's select all).
 * - `only`: just `ids` (after the header's select none).
 *
 * The sets only ever hold rows the user flipped by hand.
 */
export type OpSelection =
  | { mode: 'fromStored'; exclude: SvelteSet<number>; include: SvelteSet<number> }
  | { mode: 'allExcept' | 'only'; ids: SvelteSet<number> }

/** The stored ticks, nothing flipped: the state a group opens in. */
export function storedSelection(): OpSelection {
  return { mode: 'fromStored', exclude: new SvelteSet<number>(), include: new SvelteSet<number>() }
}

/**
 * The selection as approve sends it, with every op whose file is gone turned off: it can't run,
 * and the row showed it unticked. Still flips over the base, so the wire carries only the ops the
 * user flipped plus the few gone ones the list has drawn, at any group size.
 */
export function selectionWire(selection: OpSelection, gone: Iterable<SuggestedOpView>): WireOpSelection {
  // A retired op never runs whatever the wire says, so it needs no turning off.
  const missing = [...gone].filter((op) => !isRetiredOp(op))
  if (selection.mode === 'fromStored') {
    const exclude = new SvelteSet(selection.exclude)
    const include = new SvelteSet(selection.include)
    for (const op of missing) {
      if (op.status === 'excluded') include.delete(op.opId)
      else exclude.add(op.opId)
    }
    return { mode: 'fromStored', excludeOpIds: [...exclude], includeOpIds: [...include] }
  }
  const ids = new SvelteSet(selection.ids)
  for (const op of missing) {
    if (selection.mode === 'allExcept') ids.add(op.opId)
    else ids.delete(op.opId)
  }
  return { mode: selection.mode, opIds: [...ids] }
}

/** Whether another suggestion took this op's file over (a newer one names it, or one that ran
 *  already changed it). Its row shows why, and nothing ever ticks it: the store never runs it. */
export function isRetiredOp(op: SuggestedOpView): boolean {
  return op.status === 'superseded' || op.status === 'handledElsewhere'
}

/** Whether the user's flips over the base tick `op`, before anything the disk says about it. A
 *  retired op (`isRetiredOp`) is never ticked, whatever the flips say. */
export function flippedOn(selection: OpSelection, op: SuggestedOpView): boolean {
  if (isRetiredOp(op)) return false
  switch (selection.mode) {
    case 'fromStored':
      return op.status === 'excluded' ? selection.include.has(op.opId) : !selection.exclude.has(op.opId)
    case 'allExcept':
      return !selection.ids.has(op.opId)
    case 'only':
      return selection.ids.has(op.opId)
  }
}
