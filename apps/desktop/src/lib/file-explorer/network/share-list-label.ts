/**
 * What a person calls the server a share-list path names: the name the hub shows it by
 * (its Bonjour name for a server mDNS found, the name it was saved with for a manual
 * one, both of which reach the discovery state), else the path's own key. The tab label
 * and the error screen read it, ❌ never the path's host key, which is identity only.
 */

import { sameShareList, shareListHostKey, shareListPath } from '#lib/servers/share-list-path.js'
import { getNetworkHosts } from './network-store.svelte'

/** The friendly name of the server `path` lists, or `null` when `path` is no share list. */
export function shareListServerLabel(path: string): string | null {
  const address = shareListHostKey(path)
  if (address === null) return null
  const host = getNetworkHosts().find((candidate) => sameShareList(shareListPath(candidate), path))
  return host?.name ?? address.key
}
