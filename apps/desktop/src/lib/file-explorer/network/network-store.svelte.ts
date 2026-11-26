/**
 * Network discovery store: the hosts the backend knows, kept in sync at app level.
 *
 * The backend browses mDNS only while something needs it; this store's part is to say
 * when a Servers view is on screen (`holdDiscoveryForServersView`). Between browses the
 * backend keeps the hosts it found, so a view opens on known servers straight away.
 */

import { SvelteSet, SvelteMap } from 'svelte/reactivity'
import {
  listNetworkHosts,
  getNetworkDiscoveryState,
  resolveNetworkHost,
  onNetworkHostFound,
  onNetworkHostLost,
  onNetworkHostResolved,
  onNetworkDiscoveryStateChanged,
  hasCachedSmbCredentials,
  deleteSmbCredentials,
  setServersViewShown,
} from '#lib/tauri-commands/index.js'
import type { UnlistenFn } from '#lib/tauri-commands/index.js'
import type { NetworkHost, DiscoveryState } from '../types'
import { shareListHostKey } from '#lib/servers/share-list-path.js'
import type { SignedInAs } from './signed-in-as'

// Singleton state for network discovery
let hosts = $state<NetworkHost[]>([])
let discoveryState = $state<DiscoveryState>('idle')
const resolvingHosts = new SvelteSet<string>()

/**
 * The account each host's share list last signed in as, by host id. ❗ Off the
 * listings only: a guest listing says guest, a sign-in says its account
 * (`setListedAccount`), and an account listing answered from the backend's cache
 * keeps what the sign-in recorded, since it IS that listing.
 */
const listedAccounts = new SvelteMap<string, SignedInAs>()
/** Hosts a guest listing worked on this session: where "Browse as guest" is a real way back. */
const guestListingHosts = new SvelteSet<string>()
/**
 * The host the backend's share-list resolver named for a share list no discovered host
 * stands for (a server saved without contact, which discovery never lists), by the path's
 * server key and port. Filled from the backend's own answer, ❌ never made up here: its id
 * is the one the share cache and the session credential are keyed by.
 */
const resolvedShareListHosts = new SvelteMap<string, NetworkHost>()
/** How many Servers views are on screen (two panes can each show one). */
let serversViewsShown = 0

// Credential status tracking - 'unknown' | 'has_creds' | 'no_creds' | 'failed'
type CredentialStatus = 'unknown' | 'has_creds' | 'no_creds' | 'failed'
const credentialStatuses = new SvelteMap<string, CredentialStatus>()

// Event listeners
let unlistenHostFound: UnlistenFn | undefined
let unlistenHostLost: UnlistenFn | undefined
let unlistenHostResolved: UnlistenFn | undefined
let unlistenStateChanged: UnlistenFn | undefined
let initialized = false

/**
 * Start resolution for a host (fire-and-forget, non-blocking): its hostname and address.
 * ❌ Nothing here lists its shares (#324): a share list is read when a pane opens it.
 */
function startResolution(host: NetworkHost) {
  // Skip if already resolved or already resolving
  if (host.hostname || resolvingHosts.has(host.id)) {
    return
  }

  // Mark as resolving
  resolvingHosts.add(host.id)

  // Fire and forget - don't await, don't block UI
  resolveNetworkHost(host.id)
    .then((resolved) => {
      if (resolved) {
        hosts = hosts.map((h) => (h.id === host.id ? resolved : h))
      }
    })
    .catch(() => {
      // Resolution failed, just leave as unresolved
    })
    .finally(() => {
      resolvingHosts.delete(host.id)
    })
}

/**
 * Keeps the backend's mDNS browse running while a Servers list is on screen, so hosts
 * arrive and leave live there. Call when the list appears; call the returned release when
 * it goes. ❌ It lists no server's shares: the Servers list never dials (#324). The backend
 * hears only the first appearance and the last departure.
 */
export function holdDiscoveryForServersView(): () => void {
  serversViewsShown += 1
  if (serversViewsShown === 1) {
    void setServersViewShown(true)
  }
  let released = false
  return () => {
    if (released) return
    released = true
    serversViewsShown -= 1
    if (serversViewsShown === 0) void setServersViewShown(false)
  }
}

/**
 * Initialize network discovery - call once at app startup.
 * Subscribes to network events and loads initial hosts.
 */
export async function initNetworkDiscovery(): Promise<void> {
  if (initialized) return
  initialized = true

  // A reloaded page starts from zero views, while the backend may still hold the browse
  // for the page before it: send this page's truth.
  void setServersViewShown(serversViewsShown > 0)

  // Load initial data
  hosts = await listNetworkHosts()
  discoveryState = await getNetworkDiscoveryState()

  // Start resolving every loaded host that has no hostname yet (non-blocking).
  for (const host of hosts) startResolution(host)

  // Subscribe to events
  unlistenHostFound = await onNetworkHostFound((host) => {
    hosts = [...hosts.filter((h) => h.id !== host.id), host]
    startResolution(host)
  })

  unlistenHostLost = await onNetworkHostLost((id) => {
    hosts = hosts.filter((h) => h.id !== id)
    listedAccounts.delete(id)
  })

  // Listen for host resolution from mDNS (Bonjour NSNetService.resolve())
  unlistenHostResolved = await onNetworkHostResolved((resolved) => {
    // Update the host with resolved info (hostname and IP from mDNS)
    hosts = hosts.map((h) => (h.id === resolved.id ? { ...h, ...resolved } : h))
  })

  unlistenStateChanged = await onNetworkDiscoveryStateChanged((state) => {
    discoveryState = state
  })
}

/**
 * Cleanup network discovery - call on app shutdown.
 */
export function cleanupNetworkDiscovery(): void {
  unlistenHostFound?.()
  unlistenHostLost?.()
  unlistenHostResolved?.()
  unlistenStateChanged?.()
  initialized = false
}

/**
 * Get reactive network hosts array, sorted alphabetically by name.
 */
export function getNetworkHosts(): NetworkHost[] {
  return [...hosts].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' }))
}

/**
 * Get reactive discovery state.
 */
export function getDiscoveryState(): DiscoveryState {
  return discoveryState
}

// ============================================================================
// Share lists' servers and accounts
// ============================================================================

/** The share-list map key of `path`: its server key and port, or `null` for no share list. */
function shareListMapKey(path: string): string | null {
  const address = shareListHostKey(path)
  return address ? `${address.key}:${String(address.port)}` : null
}

/** The host the backend resolved the share list at `path` to, when one was noted. */
export function getResolvedShareListHost(path: string): NetworkHost | undefined {
  const key = shareListMapKey(path)
  return key === null ? undefined : resolvedShareListHosts.get(key)
}

/** Notes the host the backend resolved the share list at `path` to. */
export function noteResolvedShareListHost(path: string, host: NetworkHost): void {
  const key = shareListMapKey(path)
  if (key !== null) resolvedShareListHosts.set(key, host)
}

/**
 * Drops the listed accounts of every host that is the same machine as `host`: the
 * manual host an add just injected, plus a discovered twin at the same IP or
 * hostname.
 *
 * ❗ After an add that typed an account, a guest listing fetched earlier for the
 * discovered twin would otherwise still open, which is the listing the person
 * just said they don't want. The backend skips guest from then on; this is the
 * frontend's copy of the same answer.
 */
export function forgetShareListsOfMachine(host: NetworkHost): void {
  for (const other of hosts) {
    const sameIp = host.ipAddress !== undefined && other.ipAddress === host.ipAddress
    const sameHostname = host.hostname !== undefined && other.hostname === host.hostname
    if (other.id === host.id || sameIp || sameHostname) listedAccounts.delete(other.id)
  }
  listedAccounts.delete(host.id)
}

/** Whether a guest listing worked on `hostId` this session, so "Browse as guest" can offer a way back. */
export function guestListingWorked(hostId: string): boolean {
  return guestListingHosts.has(hostId)
}

/** The account `hostId`'s share list last signed in as, or `undefined` when no listing said. */
export function getListedAccount(hostId: string): SignedInAs | undefined {
  return listedAccounts.get(hostId)
}

/** Records the account a listing with explicit credentials (or the guest choice) signed in as. */
export function setListedAccount(hostId: string, account: SignedInAs): void {
  listedAccounts.set(hostId, account)
  if (account.kind === 'guest') guestListingHosts.add(hostId)
}

// ============================================================================
// Credential status functions
// ============================================================================

/**
 * Get credential status for a host (by server name).
 */
export function getCredentialStatus(serverName: string): CredentialStatus {
  const key = serverName.toLowerCase()
  return credentialStatuses.get(key) ?? 'unknown'
}

/**
 * Set credential status for a host.
 * Call this when credentials succeed or fail.
 */
export function setCredentialStatus(serverName: string, status: CredentialStatus): void {
  const key = serverName.toLowerCase()
  credentialStatuses.set(key, status)
}

/**
 * Marks a host `has_creds` when this session already READ its server-level password (a
 * listing's own lookup, a mount), from the backend's in-memory cache.
 *
 * ❗ Never a Keychain access: each can raise a system prompt, and the question (should
 * the share list offer "Forget saved password"?) is not worth one. So an unread
 * password leaves the status as it was, and the button waits for something to read it.
 */
export async function noteCachedCredentials(serverName: string): Promise<void> {
  if (await hasCachedSmbCredentials(serverName)) credentialStatuses.set(serverName.toLowerCase(), 'has_creds')
}

/**
 * Delete stored credentials for a host and update status.
 * Removes server-level credentials from the Keychain/credential store.
 */
export async function forgetCredentials(serverName: string): Promise<void> {
  await deleteSmbCredentials(serverName, null)
  credentialStatuses.set(serverName.toLowerCase(), 'no_creds')
}
