/**
 * What a person calls the server a share-list path names: the name the hub shows it by
 * (its Bonjour name for a server mDNS found, the name it was saved with for a manual
 * one, both of which reach the discovery state), else the path's own key. The tab label
 * and the error screen read it, ❌ never the path's host key, which is identity only.
 */

import { isShareListLocation, sameShareList, shareListHostKey, shareListPath } from '#lib/servers/share-list-path.js'
import { fetchShares, getNetworkHosts, setListedAccount } from './network-store.svelte'
import type { ShareListAccount } from '#lib/ipc/bindings.js'
import type { ShareListServer } from './share-list-sign-in'
import type { NetworkHost } from '../types'

/** The friendly name of the server `path` lists, or `null` when `path` is no share list. */
export function shareListServerLabel(path: string): string | null {
  const address = shareListHostKey(path)
  if (address === null) return null
  return shareListHost(path)?.name ?? address.key
}

/**
 * The server `path` lists, as the sign-in module takes it (its host and what to call it), or
 * `null` when `path` is no share list or names a server the discovery state doesn't hold
 * (an account action needs a host to sign in to).
 */
export function shareListServerOf(path: string): ShareListServer | null {
  const host = shareListHost(path)
  return host ? { host, label: host.name } : null
}

/**
 * The server whose share list a pane on `volumeId` at `path` shows, or `null` (another
 * place, or a server the discovery state doesn't hold). What Edit server and the servers
 * commands aim at on a share list: the path is the one record of which server it is.
 */
export function shareListHostAt(volumeId: string, path: string): NetworkHost | null {
  return isShareListLocation(volumeId, path) ? shareListHost(path) : null
}

/**
 * Notes who a share list's listing signed in as (`listing-complete`'s `shareListAccount`),
 * so the path bar's account and the hub's row say the same, the way a sheet's listing does.
 */
export function noteShareListAccount(path: string, account: ShareListAccount): void {
  const host = shareListHost(path)
  if (host) setListedAccount(host.id, account)
}

/**
 * A share list landed at `path`: the hub's record of that server's shares (its row's share
 * count, a saved server's share rows) reads the share cache the listing just filled, the
 * way an earlier visit to the hub left it. ❗ A cache read, ❌ never a second sign-in: the
 * backend answers from the cache the pane's listing primed, by the same host id.
 */
export function noteShareListListed(path: string): void {
  const host = shareListHost(path)
  if (host) void fetchShares(host).catch(() => {})
}

/** The discovery state's host whose share-list path is `path`, the same key the path was written from. */
function shareListHost(path: string): NetworkHost | null {
  return getNetworkHosts().find((candidate) => sameShareList(shareListPath(candidate), path)) ?? null
}
