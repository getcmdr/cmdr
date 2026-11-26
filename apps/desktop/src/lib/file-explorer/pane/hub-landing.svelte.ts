/**
 * Where a pane's Servers hub puts its cursor when it comes back from a server's share
 * list: on THAT server's row, by the share-list path the pane left.
 *
 * ❗ The landing is DERIVED, ❌ never threaded through each caller. Every way back to the
 * hub (`..`, `⌘↑`, Backspace on an error screen, history Back, a Cancel that steps out)
 * ends in the tab's path changing from `smb://<server>` to `smb://`, and `notePathChange`
 * runs at that one write (`pane-accessors.svelte.ts::setPanePath`, the only path setter
 * every commit goes through). A restore has no path to leave, so
 * `initialization.ts` sets `landOnServer` itself.
 *
 * The hub takes the landing once (`takeHubLanding`), so a later visit to the hub
 * doesn't jump the cursor again; any other path change drops it first.
 */

import { getActiveTab } from '../tabs/tab-state-manager.svelte'
import type { TabState } from '../tabs/tab-types'
import { explorerState } from './explorer-state.svelte'
import { NETWORK_VOLUME_PATH } from './navigate-refusals'
import { capabilitiesForPane } from './volume-capabilities'

/**
 * Writes `path` onto `tab`, noting the hub landing when the write takes the tab from a
 * share list to the hub, and dropping a pending one on any other change. A write that
 * doesn't change the path keeps it: a re-commit of the same `smb://` mustn't spend a
 * restored landing before the hub has rendered.
 */
export function writeTabPath(tab: TabState, path: string): void {
  if (path !== tab.path) {
    const leavingShareList =
      path === NETWORK_VOLUME_PATH &&
      tab.volumeId === 'network' &&
      capabilitiesForPane(tab.volumeId, tab.path).kind === 'share-list'
    tab.landOnServer = leavingShareList ? tab.path : undefined
  }
  tab.path = path
}

/** The share-list path the pane's hub should land on, without taking it. Reactive. */
export function hubLandingFor(paneId: 'left' | 'right'): string | undefined {
  return getActiveTab(explorerState.getTabMgr(paneId)).landOnServer
}

/** Takes the pane's pending hub landing: it's spent once the hub has it. */
export function takeHubLanding(paneId: 'left' | 'right'): string | undefined {
  const tab = getActiveTab(explorerState.getTabMgr(paneId))
  const landing = tab.landOnServer
  tab.landOnServer = undefined
  return landing
}
