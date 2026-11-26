/**
 * Which server a share-list path names, and what a person calls it: the host discovery
 * holds under that path (its Bonjour name for a server mDNS found), else the server a
 * mounted share's `..` leads to (`share-parent-row.svelte.ts`: a share macOS mounted by an
 * address discovery never saw, a Tailscale IP), else the host the backend's listing
 * resolved (a server saved without contact: `resolveShareListServer`), else just the
 * path's own key for the label. A manual host takes the name `..` gave it when there is one (the name the person
 * typed for it), since discovery names it after its address. The tab label and the error
 * screen read it, ❌ never the path's host key, which is identity only.
 */

import { isShareListLocation, sameShareList, shareListHostKey, shareListPath } from '#lib/servers/share-list-path.js'
import { shareListHost as resolveShareListHost } from '#lib/tauri-commands/index.js'
import {
  getNetworkHosts,
  getResolvedShareListHost,
  noteResolvedShareListHost,
  setListedAccount,
} from './network-store.svelte'
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
 * so the path bar's account and the Servers row say the same, the way a sheet's listing does.
 */
export function noteShareListAccount(path: string, account: ShareListAccount): void {
  const host = shareListHost(path)
  if (host) setListedAccount(host.id, account)
}

/**
 * A share list landed at `path`: when no host here stands for its server, the backend
 * names the one its listing resolved (`resolveShareListServer`), so the path bar's account
 * and the share rows' menus have a server to act on.
 */
export function noteShareListListed(path: string): void {
  if (!shareListServer(path)) void resolveShareListServer(path)
}

/**
 * The server `path` lists, asking the backend when nothing here holds a host for it (a
 * server saved without contact, which discovery never lists): the host the backend's own
 * listing resolved, ❗ so by the id its share cache and session credential are keyed by, ❌
 * never one made up from the path (a saved address keeps its case; the path's key doesn't).
 * Memory and the stores only. `null` when nothing knows the server.
 */
export async function resolveShareListServer(path: string): Promise<ShareListServer | null> {
  const known = shareListServer(path)
  if (known || shareListHostKey(path) === null) return known
  const host = await resolveShareListHost(path).catch(() => null)
  if (!host) return null
  noteResolvedShareListHost(path, host)
  return { host, label: host.name }
}

function shareListHost(path: string): NetworkHost | null {
  return shareListServer(path)?.host ?? null
}

/**
 * The server whose share-list path is `path`, by the same key the path was written from.
 * ❗ Discovery's host first: the backend resolves to it too, and caches the shares and the
 * sign-in under its id. Last, a host the backend resolved for a server discovery doesn't list.
 */
function shareListServer(path: string): ShareListServer | null {
  const named = (host: NetworkHost) => sameShareList(shareListPath(host), path)
  const outOfShare = shareParentServers().find((server) => named(server.host)) ?? null
  const host = getNetworkHosts().find(named)
  if (!host) return outOfShare ?? resolvedServer(path)
  return { host, label: host.source === 'manual' && outOfShare ? outOfShare.label : host.name }
}

function resolvedServer(path: string): ShareListServer | null {
  const host = getResolvedShareListHost(path)
  return host ? { host, label: host.name } : null
}
