/**
 * The words a pane's kind gives its list (`../views/list-words.ts`), read off the kind so
 * the views and the footer never ask which kind they show. Only a server's share list has
 * any today: its Ext column holds the share's description, its `..` goes back to Servers,
 * and its footer counts shares.
 */

import { tString } from '#lib/intl/messages.svelte.js'
import { formatInteger } from '#lib/intl/number-format.js'
import type { ListWords } from '../views/list-words'
import type { VolumeKind } from './volume-capabilities'

/** The list's words on a pane of `kind`, or `undefined` for a folder's own. */
export function listWordsFor(kind: VolumeKind): ListWords | undefined {
  if (kind !== 'share-list') return undefined
  return {
    extHeader: tString('fileExplorer.columns.description'),
    parentRowName: tString('fileExplorer.network.share.parentRowName'),
    emptyText: tString('fileExplorer.network.share.emptyList'),
    countRows: (count) => tString('fileExplorer.network.share.shareCount', { count, countText: formatInteger(count) }),
  }
}
