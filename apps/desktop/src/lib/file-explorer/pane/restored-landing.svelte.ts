/**
 * Where a restored Servers tab puts its cursor on its first listing: the tab was on a
 * server's share list when the app quit, and a restore brings it back to the Servers list
 * (nothing lists a server at launch, #324), landing on THAT server's row by its path
 * (`initialization.ts` sets `TabState.landOnServer`). Spent once; ❌ never persisted.
 *
 * A live `..` from a share list needs none of this: it loads `smb://` with the share list's
 * path to select (`FilePane`'s `leaveShareList`).
 */

import { getActiveTab } from '../tabs/tab-state-manager.svelte'
import { explorerState } from './explorer-state.svelte'

/** Takes the pane's pending restored landing: the share-list path whose Servers row the cursor goes to. */
export function takeRestoredLanding(paneId: 'left' | 'right'): string | undefined {
  const tab = getActiveTab(explorerState.getTabMgr(paneId))
  const landing = tab.landOnServer
  tab.landOnServer = undefined
  return landing
}
