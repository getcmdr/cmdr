/**
 * The words a pane's kind gives its list (`../views/list-words.ts`) and its F-key bar, read
 * off the kind so the views, the footer, and the bar never ask which kind they show.
 *
 * - A server's share list: its Ext column holds the share's description (measured as prose),
 *   its rows carry no dates and no sizes, its `..` goes back to Servers, and its footer
 *   counts shares.
 * - The Servers list (`network` at `smb://`): its Ext column is each server's Details
 *   (prose), its date is "Last used", it has no Size, its footer counts servers with the
 *   nearby search's state (`network-store.svelte.ts`, read live), and an empty list names F7.
 */

import { tString } from '#lib/intl/messages.svelte.js'
import { formatInteger } from '#lib/intl/number-format.js'
import { getNetworkEnabled } from '#lib/settings/reactive-settings.svelte.js'
import type { MessageKey } from '#lib/intl/keys.gen.js'
import { getDiscoveryState } from '../network/network-store.svelte'
import type { ListWords } from '../views/list-words'
import type { VolumeKind } from './volume-capabilities'

/** The list's words on a pane of `kind`, or `undefined` for a folder's own. */
export function listWordsFor(kind: VolumeKind): ListWords | undefined {
  if (kind === 'share-list') {
    return {
      extHeader: tString('fileExplorer.columns.description'),
      extIsProse: true,
      rowsHaveNoDates: true,
      rowsHaveNoSizes: true,
      parentRowName: tString('fileExplorer.network.share.parentRowName'),
      emptyText: tString('fileExplorer.network.share.emptyList'),
      countRows: (count) =>
        tString('fileExplorer.network.share.shareCount', { count, countText: formatInteger(count) }),
    }
  }
  if (kind === 'network') {
    return {
      extHeader: tString('fileExplorer.columns.details'),
      extIsProse: true,
      dateHeader: tString('fileExplorer.columns.lastUsed'),
      rowsHaveNoSizes: true,
      emptyText: tString('fileExplorer.network.servers.emptyList'),
      countRows: (count) => tString(serverCountKey(), { count, countText: formatInteger(count) }),
    }
  }
  return undefined
}

/** "5 servers", plus where the nearby search stands: still looking, or turned off. */
function serverCountKey(): MessageKey {
  if (!getNetworkEnabled()) return 'fileExplorer.network.servers.serverCountDiscoveryOff'
  return getDiscoveryState() === 'searching'
    ? 'fileExplorer.network.servers.serverCountSearching'
    : 'fileExplorer.network.servers.serverCount'
}

/** One F-key button the kind relabels: its words, and whether it acts here. */
export interface FunctionKeyWord {
  label: string
  enabled: boolean
}

/**
 * The F-key bar's per-kind slots. A slot left out keeps the folder's own label and the
 * capability rule's enabled state.
 */
export interface FunctionKeyWords {
  /** F2 acts here though nothing is writable: Edit server, on the Servers list and a share list. */
  renameEnabled?: boolean
  /** F7. */
  newFolder?: FunctionKeyWord
  /** F8. */
  delete?: FunctionKeyWord
}

/**
 * What the F-key bar says on a pane of `kind`. The Servers list: F7 adds a server, F8 forgets
 * the one under the cursor (`readOnlyRefusal`'s sibling arms in the command handlers), F2 edits
 * it. A share list: F2 edits its server; F7 and F8 stay refusals, so they keep their words.
 */
export function functionKeyWordsFor(kind: VolumeKind): FunctionKeyWords {
  if (kind === 'network') {
    return {
      renameEnabled: true,
      newFolder: { label: tString('fileExplorer.network.servers.addServerKey'), enabled: true },
      delete: { label: tString('fileExplorer.network.servers.forgetKey'), enabled: true },
    }
  }
  if (kind === 'share-list') return { renameEnabled: true }
  return {}
}
