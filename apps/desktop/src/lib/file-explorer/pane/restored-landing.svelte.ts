/**
 * Where a tab's next listing of one folder puts its cursor, once (`TabState.landOn`): a row
 * found by its PATH, for a door that sends the pane to another volume, whose listing the
 * pane loads on its own (the volume switch's `initialPath`), so no `selectPath` can ride it.
 *
 * - A restored Servers tab: the tab was on a server's share list when the app quit, and a
 *   restore brings it back to the Servers list (nothing lists a server at launch, #324),
 *   landing on THAT server's row (`initialization.ts` queues it).
 * - `..` out of a server's top folder onto another volume: an SFTP place's to the Servers
 *   list, a bucket's to its account's bucket list (`top-folder-parent-pane.svelte.ts`).
 *
 * Spent by the first listing of its folder, and ❌ never persisted. A listing of any OTHER
 * folder leaves it alone, so a refused switch can't drop the cursor on a row somewhere else.
 */

import { getActiveTab } from '../tabs/tab-state-manager.svelte'
import { explorerState } from './explorer-state.svelte'

/** Queues the pane's landing: its next listing of `path` puts the cursor on the row at `selectPath`. */
export function queueLanding(paneId: 'left' | 'right', landing: { path: string; selectPath: string }): void {
  getActiveTab(explorerState.getTabMgr(paneId)).landOn = landing
}

/** Takes the pane's landing for a listing of `path`: the row path the cursor goes to, or `undefined`. */
export function takeLanding(paneId: 'left' | 'right', path: string): string | undefined {
  const tab = getActiveTab(explorerState.getTabMgr(paneId))
  const landing = tab.landOn
  if (landing?.path !== path) return undefined
  tab.landOn = undefined
  return landing.selectPath
}
