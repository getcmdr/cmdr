/**
 * Which server a share-list path names, and what a person calls it: the host discovery
 * holds under that path (its Bonjour name for a server mDNS found), else the server a
 * mounted share's `..` leads to (`share-parent-row.svelte.ts`: a share macOS mounted by an
 * address discovery never saw, a Tailscale IP), else just the path's own key for the
 * label. A manual host takes the name `..` gave it when there is one (the name the person
 * typed for it), since discovery names it after its address. The tab label and the error
 * screen read it, ❌ never the path's host key, which is identity only.
 */

import { isShareListLocation, sameShareList, shareListHostKey, shareListPath } from '#lib/servers/share-list-path.js'
import { fetchShares, getNetworkHosts, setListedAccount } from './network-store.svelte'
import { shareParentServers } from './share-parent-row.svelte'
import type { ShareListAccount } from '#lib/ipc/bindings.js'
import type { ShareListServer } from './share-list-sign-in'
import type { NetworkHost } from '../types'

/** The friendly name of the server `path` lists, or `null` when `path` is no share list. */
export function shareListServerLabel(path: string): string | null {
  const address = shareListHostKey(path)
  if (address === null) return null
  return shareListServer(path)?.label ?? address.key
}

/**
 * The server `path` lists, as the sign-in module takes it (its host and what to call it), or
 * `null` when `path` is no share list or names a server nothing on the frontend holds (an
 * account action needs a host to sign in to).
 */
export function shareListServerOf(path: string): ShareListServer | null {
  return shareListServer(path)
}

/**
 * The server whose share list a pane on `volumeId` at `path` shows, or `null` (another
 * place, or a server nothing on the frontend holds). What Edit server and the servers
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

function shareListHost(path: string): NetworkHost | null {
  return shareListServer(path)?.host ?? null
}

/**
 * The server whose share-list path is `path`, by the same key the path was written from.
 * ❗ Discovery's host first: the backend resolves to it too, and caches the shares and the
 * sign-in under its id.
 */
function shareListServer(path: string): ShareListServer | null {
  const named = (host: NetworkHost) => sameShareList(shareListPath(host), path)
  const outOfShare = shareParentServers().find((server) => named(server.host)) ?? null
  const host = getNetworkHosts().find(named)
  if (!host) return outOfShare
  return { host, label: host.source === 'manual' && outOfShare ? outOfShare.label : host.name }
}
