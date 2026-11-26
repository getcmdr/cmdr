/**
 * "Copy path from <source> to <target> pane": mirror the source pane's location
 * (volume + path) into the target pane WITHOUT shifting keyboard focus. Lifted out of
 * `DualPaneExplorer`; the component keeps the one-line `export function
 * copyPathBetweenPanes` delegate.
 *
 * When the source pane is focused, the cursor refines the destination:
 * cursor-on-folder uses the folder's path; cursor-on-server (the Servers hub) opens that
 * server's share list; cursor-on-share (a server's share list) opens the same share list
 * and mounts that share once it lands (`share-list-arrival.svelte.ts`), ❌ never the
 * share's `smb://` path, which is mounted, not navigated to. `followCursor: false` skips
 * that refinement: Clone pane (`pane.clone`, ⌘⇧C, Total Commander's "Target = Source")
 * copies the location exactly. All navigation uses `source: 'mirror'`, which keeps focus
 * on the source pane (no focus shift, L1); `restoreFocus` re-anchors STORE focus to
 * wherever it was, so the user keeps working where they were.
 */

import type { NetworkHost } from '../types'
import type { NavigateIntent, NavigateResult } from './navigate'
import type { CopyPathBetweenPanesArgs, FilePaneAPI } from './types'
import { capabilitiesForPane } from './volume-capabilities'
import { shareListHostAt } from '../network/share-list-label'
import { NETWORK_VOLUME_PATH } from './navigate-refusals'

export interface PaneMirrorDeps {
  navigate: (intent: NavigateIntent) => NavigateResult
  getPaneRef: (pane: 'left' | 'right') => FilePaneAPI | undefined
  getPaneVolumeId: (pane: 'left' | 'right') => string
  getPanePath: (pane: 'left' | 'right') => string
  getFocusedPane: () => 'left' | 'right'
  setFocusedPane: (pane: 'left' | 'right') => void
}

export interface PaneMirror {
  copyPathBetweenPanes: (args: CopyPathBetweenPanesArgs) => void
  /** Takes `target` to `host`'s share list, mounting `mount` once it lands when named, keeping focus where it is. */
  openShareList: (target: 'left' | 'right', host: NetworkHost, mount: string | undefined) => void
}

export function createPaneMirror(deps: PaneMirrorDeps): PaneMirror {
  /** Restore focus to a pane after a target-pane state change so the user keeps working where they were. */
  function restoreFocus(pane: 'left' | 'right'): void {
    if (deps.getFocusedPane() !== pane) {
      deps.setFocusedPane(pane)
      // focusedPane persistence fires from the subscriber's focus effect.
    }
  }

  /** Mirror a {volumeId, path} state to a target pane without shifting focus. */
  function mirrorLocalStateToPane(target: 'left' | 'right', volumeId: string, path: string): void {
    const originalFocused = deps.getFocusedPane()
    // Keep the same-volume same-path no-op: routing it through `{ location }`
    // would `navigateToPath(samePath)` — a redundant listing reload with
    // cursor/selection churn. The `{ location }` arm subsumes the other two
    // branches (cross-volume → switch, same-volume different path → in-place).
    if (deps.getPaneVolumeId(target) === volumeId && deps.getPanePath(target) === path) {
      restoreFocus(originalFocused)
      return
    }
    // `source: 'mirror'` keeps focus on the source pane (no focus shift, L1).
    deps.navigate({ pane: target, to: { goTo: { volumeId, path } }, source: 'mirror' })
    restoreFocus(originalFocused)
  }

  /** The Servers hub in `target`: a volume select, which always lands on the hub, never a remembered share list. */
  function mirrorHub(target: 'left' | 'right'): void {
    const originalFocused = deps.getFocusedPane()
    deps.navigate({
      pane: target,
      to: { selectVolume: { volumeId: 'network', path: NETWORK_VOLUME_PATH } },
      source: 'mirror',
    })
    restoreFocus(originalFocused)
  }

  /** Opens `host`'s share list in `target` without shifting focus, mounting `mount` once it lands. */
  function openShareList(target: 'left' | 'right', host: NetworkHost, mount: string | undefined): void {
    const originalFocused = deps.getFocusedPane()
    deps.getPaneRef(target)?.openShareList(host, { mount })
    restoreFocus(originalFocused)
  }

  function copyPathBetweenPanes({ source, target, followCursor }: CopyPathBetweenPanesArgs): void {
    // The cursor refines the destination only on a focused source, and only when asked.
    const refineByCursor = followCursor !== false && deps.getFocusedPane() === source
    mirrorPane(source, target, refineByCursor)
  }

  function mirrorPane(source: 'left' | 'right', target: 'left' | 'right', sourceFocused: boolean): void {
    if (source === target) return
    const sourcePaneRef = deps.getPaneRef(source)
    if (!sourcePaneRef) return

    const sourceVolumeId = deps.getPaneVolumeId(source)
    const sourcePath = deps.getPanePath(source)
    const sourceKind = capabilitiesForPane(sourceVolumeId, sourcePath).kind

    // The Servers hub: a server under the cursor opens its share list, anything else the hub.
    if (sourceKind === 'network') {
      const cursor = sourceFocused ? sourcePaneRef.getNetworkCursorEntry() : null
      if (cursor?.kind === 'host') openShareList(target, cursor.host, undefined)
      else mirrorHub(target)
      return
    }

    const entry = sourceFocused ? sourcePaneRef.getCursorEntry() : null

    // A server's share list: the same list, mounting the share under the cursor.
    if (sourceKind === 'share-list') {
      const host = shareListHostAt(sourceVolumeId, sourcePath)
      const share = entry && entry.name !== '..' ? entry.name : undefined
      if (host) openShareList(target, host, share)
      else mirrorLocalStateToPane(target, sourceVolumeId, sourcePath)
      return
    }

    // Normal listing on the source: cursor-on-folder refines the path.
    const destPath = entry && entry.isDirectory && entry.name !== '..' ? entry.path : sourcePath
    mirrorLocalStateToPane(target, sourceVolumeId, destPath)
  }

  return { copyPathBetweenPanes, openShareList }
}
