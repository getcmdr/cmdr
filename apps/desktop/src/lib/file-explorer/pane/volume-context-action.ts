/**
 * The native breadcrumb context menu's "Eject (name)" / server-row handler,
 * split out of `DualPaneExplorer.svelte`'s `onMount` to keep it under its
 * length cap: pure orchestration logic, so a plain `.ts` helper.
 *
 * Handles the `volume-context-action` event (see `on_menu_event` in `lib.rs`).
 * The Svelte popup paths in `VolumeBreadcrumb` call `ejectVolume` directly;
 * this only covers the native-menu case.
 */
import type { VolumeContextAction } from '#lib/tauri-commands/index.js'
import { ejectAllAndSay, ejectAndSay } from '../navigation/detach-volume'
import { pathForPickedVolume } from '../navigation/picked-volume-path'
import { runServerRowAction } from '../navigation/server-row-actions'
import type { NavigateIntent, NavigateResult } from './navigate'
import type { VolumeInfo } from '../types'

export interface VolumeContextActionDeps {
  getVolumes: () => VolumeInfo[]
  getFocusedPane: () => 'left' | 'right'
  navigate: (intent: NavigateIntent) => NavigateResult
}

export function handleVolumeContextAction(payload: VolumeContextAction, deps: VolumeContextActionDeps): void {
  if (payload.action === 'eject') {
    void ejectAndSay(payload.volumeId, payload.volumeName)
    return
  }
  if (payload.action === 'eject-all') {
    void ejectAllAndSay(payload.volumeId)
    return
  }
  // Everything a SERVER row's menu offers (Open, Edit…, Disconnect,
  // the pin pair, the two Forgets) is dispatched from one module,
  // shared with the Servers list and the palette.
  void runServerRowAction({
    ...payload,
    // ❗ Open needs a pane to move, and the `navigate()`
    // transaction lives here. Through the same `selectVolume`
    // intent a switcher click raises, so the pinned-tab fork,
    // focus, and history push all apply.
    onOpen: (volumeId: string) => {
      const volume = deps.getVolumes().find((v) => v.id === volumeId)
      if (!volume) return
      deps.navigate({
        pane: deps.getFocusedPane(),
        to: { selectVolume: { volumeId, path: pathForPickedVolume(volume) } },
        source: 'user',
      })
    },
  })
}
