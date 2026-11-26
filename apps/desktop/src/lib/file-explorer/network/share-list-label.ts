/**
 * What a person calls the server a share-list path names: the name the hub shows it by
 * (its Bonjour name for a server mDNS found, the name it was saved with for a manual
 * one, both of which reach the discovery state), else the path's own key. The tab label
 * and the error screen read it, ❌ never the path's host key, which is identity only.
 */

import { sameShareList, shareListHostKey, shareListPath } from '#lib/servers/share-list-path.js'
import { getNetworkHosts } from './network-store.svelte'
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

/** The discovery state's host whose share-list path is `path`, the same key the path was written from. */
function shareListHost(path: string): NetworkHost | null {
  return getNetworkHosts().find((candidate) => sameShareList(shareListPath(candidate), path)) ?? null
}
