/**
 * The first network action, and every one after it.
 *
 * mDNS browsing doesn't run at app launch on fresh installs. The first user action that
 * depends on networking calls `triggerNetworkDiscovery()`, which runs the backend's
 * existing-mount upgrade pass and records `network.firstTriggerDone = true`, so later
 * launches warm the server list up briefly without surprising the user. The browse
 * itself, which is what fires the system "Cmdr wants to find devices on local networks"
 * prompt, runs while a pane shows the Servers list (`holdDiscoveryForServersView`) or an
 * upgrade needs it.
 *
 * Callers: a pane showing the Servers list (`../pane/servers-list-pane.svelte.ts`) and the
 * OS-mount → direct-smb2 upgrade click in `VolumeBreadcrumb`.
 *
 * No-op when `network.enabled === false`. The caller doesn't need to gate; this is the
 * single chokepoint.
 */

import { noteNetworkAction } from '#lib/tauri-commands/index.js'
import { getSetting, setSetting } from '#lib/settings/index.js'

export function triggerNetworkDiscovery(): void {
  if (!getSetting('network.enabled')) return

  void noteNetworkAction()

  if (!getSetting('network.firstTriggerDone')) {
    setSetting('network.firstTriggerDone', true)
  }
}
