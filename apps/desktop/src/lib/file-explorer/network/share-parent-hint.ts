/**
 * The one-time explainer for `..` out of an SMB share's top folder (#1238). Its own
 * module, apart from `share-parent-row.svelte.ts`, because the toast's buttons call
 * back into that store.
 */

import { getSetting, setSetting } from '#lib/settings/index.js'
import { addToast } from '#lib/ui/toast/index.js'
import type { ShareParentLanding } from './share-parent-row.svelte'
import ShareParentHintToastContent from './ShareParentHintToastContent.svelte'

/**
 * The first time `..` takes someone out of a share's top folder, once the share list
 * has loaded so they see where they landed: says what happened and offers to turn it
 * off. Once per install (`behavior.shareParentRowHintSeen`), and sticky.
 */
export function showShareParentHintOnce(landing: ShareParentLanding): void {
  if (getSetting('behavior.shareParentRowHintSeen')) return
  setSetting('behavior.shareParentRowHintSeen', true)
  addToast(ShareParentHintToastContent, {
    level: 'info',
    dismissal: 'persistent',
    id: 'share-parent-row-hint',
    props: { volumeId: landing.volumeId, serverLabel: landing.serverLabel },
  })
}
