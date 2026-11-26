// Network hosts, SMB shares, keychain, and mounting

import { type UnlistenFn } from '@tauri-apps/api/event'
import { commands, events } from '#lib/ipc/bindings.js'
import { throwEjectError } from '#lib/file-explorer/navigation/eject-error.js'
import { throwMountError } from '#lib/file-explorer/network/mount-error.js'
import { throwShareListError } from '#lib/file-explorer/network/share-list-error.js'
import { throwReconnectError } from '#lib/file-explorer/network/reconnect-error.js'
import type {
  CredentialSource,
  DirectConnectionSwitch,
  MountResult,
  ShareParent as ShareParentWire,
  SignInShape,
  SignedInReply,
  SmbFellBackToOsMount,
  SmbLinkConsent,
  SmbOsMountNoticeWithdrawn,
  SystemSavedAccount,
  UpgradeResult,
} from '#lib/ipc/bindings.js'
import { throwIpcError } from './ipc-types'
import { throwAddServerError } from '#lib/servers/add-server-error.js'
import type {
  AuthOptions,
  ConnectionMode,
  DiscoveryState,
  KnownNetworkShare,
  NetworkHost,
  ShareListResult,
} from '../file-explorer/types'

export type { SignInShape }

/** Result of connecting to a manually-specified server. */
export interface ManualConnectResult {
  /** The injected network host */
  host: NetworkHost
  /** Optional share path (when user typed smb://host/share) */
  sharePath: string | null
}

// ============================================================================
// Network discovery (macOS only)
// ============================================================================

/**
 * Gets all currently discovered network hosts.
 * Only available on macOS.
 * @returns Array of NetworkHost objects
 */
export async function listNetworkHosts(): Promise<NetworkHost[]> {
  try {
    return (await commands.listNetworkHosts()) as NetworkHost[]
  } catch {
    // Command not available (non-macOS) - return empty array
    return []
  }
}

/**
 * Gets the current network discovery state.
 * Only available on macOS.
 * @returns Current DiscoveryState
 */
export async function getNetworkDiscoveryState(): Promise<DiscoveryState> {
  try {
    return await commands.getNetworkDiscoveryState()
  } catch {
    // Command not available (non-macOS) - return idle
    return 'idle'
  }
}

/**
 * Resolves a network host's hostname and IP address.
 * This performs lazy resolution - only called on hover or when connecting.
 * Only available on macOS.
 * @param hostId The host ID to resolve
 * @returns Updated NetworkHost with hostname and IP, or null if not found
 */
export async function resolveNetworkHost(hostId: string): Promise<NetworkHost | null> {
  try {
    return (await commands.resolveHost(hostId)) as NetworkHost | null
  } catch {
    // Command not available (non-macOS) - return null
    return null
  }
}

// ============================================================================
// Network discovery event listeners
// ============================================================================

/** Fires when mDNS discovers a host. Payload is the bare `NetworkHost`. */
export function onNetworkHostFound(handler: (host: NetworkHost) => void): Promise<UnlistenFn> {
  return events.networkHostFound.listen((event) => {
    handler(event.payload as NetworkHost)
  })
}

/** Fires when a host disappears. */
export function onNetworkHostLost(handler: (id: string) => void): Promise<UnlistenFn> {
  return events.networkHostLost.listen((event) => {
    handler(event.payload.id)
  })
}

/** Fires when a host's hostname / IP is resolved. Payload is the updated `NetworkHost`. */
export function onNetworkHostResolved(handler: (host: NetworkHost) => void): Promise<UnlistenFn> {
  return events.networkHostResolved.listen((event) => {
    handler(event.payload as NetworkHost)
  })
}

/** Fires when the discovery state changes (idle / searching / active). */
export function onNetworkDiscoveryStateChanged(handler: (state: DiscoveryState) => void): Promise<UnlistenFn> {
  return events.networkDiscoveryStateChanged.listen((event) => {
    handler(event.payload.state)
  })
}

// ============================================================================
// Known shares store (macOS only)
// ============================================================================

/**
 * Gets a specific known share by server and share name.
 * Only available on macOS.
 * @param serverName Server hostname or IP
 * @param shareName Share name
 * @returns KnownNetworkShare if found, null otherwise
 */
export async function getKnownShareByName(serverName: string, shareName: string): Promise<KnownNetworkShare | null> {
  try {
    return await commands.getKnownShareByName(serverName, shareName)
  } catch {
    // Command not available (non-macOS) - return null
    return null
  }
}

/**
 * Updates or adds a known network share after successful connection.
 * Only available on macOS.
 * @param serverName Server hostname or IP
 * @param shareName Share name
 * @param lastConnectionMode How we connected (guest or credentials)
 * @param lastKnownAuthOptions Available auth options
 * @param username Username used (null for guest)
 */
export async function updateKnownShare(
  serverName: string,
  shareName: string,
  lastConnectionMode: ConnectionMode,
  lastKnownAuthOptions: AuthOptions,
  username: string | null,
): Promise<void> {
  try {
    await commands.updateKnownShare(serverName, shareName, lastConnectionMode, lastKnownAuthOptions, username)
  } catch {
    // Command not available (non-macOS) - silently fail
  }
}

/**
 * The username to pre-fill for `host`, or null if it has never been signed in to.
 *
 * Takes the whole host: the backend looks the account up where the host is DIALED (its
 * hostname or IP, on its port), which its Bonjour name alone can't say. Only available on macOS.
 */
export async function getUsernameHint(host: NetworkHost): Promise<string | null> {
  try {
    return await commands.getUsernameHint(wireHost(host))
  } catch {
    // Command not available (non-macOS) - no hint to offer
    return null
  }
}

// ============================================================================
// Keychain operations (macOS only)
// ============================================================================

/**
 * Lets links on `server` sign in to `target`. Call only on a person's yes.
 * `false` when Cmdr refuses to ask about that name.
 */
export async function allowSmbLinkTarget(server: string, target: string): Promise<boolean> {
  return await commands.allowSmbLinkTarget(server, target)
}

/** Takes a pair back, so links on `server` ask again on their next call. */
export async function forgetSmbLinkTarget(server: string, target: string): Promise<boolean> {
  return await commands.forgetSmbLinkTarget(server, target)
}

/** Every pair a person allowed, for the settings screen. */
export async function listSmbLinkConsents(): Promise<SmbLinkConsent[]> {
  return await commands.listSmbLinkConsents()
}

/**
 * Saves SMB credentials to the Keychain.
 * Credentials are stored under "Cmdr" service name in Keychain Access.
 * @param server Server hostname or IP
 * @param share Optional share name (null for server-level credentials)
 * @param username Username for authentication
 * @param password Password for authentication
 */
export async function saveSmbCredentials(
  server: string,
  share: string | null,
  username: string,
  password: string,
): Promise<void> {
  const res = await commands.saveSmbCredentials(server, share, username, password)
  if (res.status === 'error') throwIpcError(res.error)
}

/** Returns whether credential storage is using an encrypted file fallback instead of the system keyring. */
export async function isUsingCredentialFileFallback(): Promise<boolean> {
  return commands.isUsingCredentialFileFallback()
}

/**
 * Whether a server-level password was already read this session (a listing or a
 * mount found it). ❗ Cache only: never touches the Keychain, so it costs no prompt.
 */
export async function hasCachedSmbCredentials(server: string): Promise<boolean> {
  return commands.hasCachedSmbCredentials(server)
}

/**
 * Deletes SMB credentials from the Keychain.
 * @param server Server hostname or IP
 * @param share Optional share name
 */
export async function deleteSmbCredentials(server: string, share: string | null): Promise<void> {
  const res = await commands.deleteSmbCredentials(server, share)
  if (res.status === 'error') throwIpcError(res.error)
}

/**
 * Lists shares on a host using provided credentials.
 * @param hostId Unique identifier for the host (used for caching)
 * @param hostname Hostname to connect to
 * @param ipAddress Optional resolved IP address
 * @param port SMB port
 * @param username Username for authentication (null for guest)
 * @param password Password for authentication (null for guest)
 * @param credentialSource `typed` for the sign-in sheet, `saved` for a Keychain entry (logged on a refusal)
 * @param timeoutMs Optional timeout in milliseconds (default: 15000)
 * @param cacheTtlMs Optional cache TTL in milliseconds (default: 30000)
 * @throws ShareListFailure carrying the typed `ShareListError`; `shareListErrorOf` gets it back
 */
export async function listSharesWithCredentials(
  hostId: string,
  hostname: string,
  ipAddress: string | undefined,
  port: number,
  username: string | null,
  password: string | null,
  credentialSource: CredentialSource,
  timeoutMs?: number,
  cacheTtlMs?: number,
): Promise<ShareListResult> {
  const res = await commands.listSharesWithCredentials(
    hostId,
    hostname,
    ipAddress ?? null,
    port,
    username,
    password,
    credentialSource,
    timeoutMs ?? null,
    cacheTtlMs ?? null,
  )
  if (res.status === 'error') throwShareListError(res.error)
  return res.data
}

// ============================================================================
// SMB mounting (macOS only)
// ============================================================================

/**
 * Mounts an SMB share to the local filesystem.
 * If the share is already mounted, returns the existing mount path without re-mounting.
 *
 * @param server Server hostname or IP address
 * @param share Name of the share to mount
 * @param username Optional username for authentication
 * @param password Optional password for authentication
 * @param timeoutMs Optional timeout in milliseconds (default: 20000)
 * @param hostName The name the person knows the server by; a mount that goes through is
 *   saved as a share place under it (`docs/specs/saved-smb-shares.md`)
 * @param attemptId Mint with `newServerAttemptId()`; `cancelServerConnect(attemptId)` calls the
 *   mount off, answering `cancelled` (or `sign_in_helper_stuck` after a long wait on a server that answers)
 * @returns MountResult with mount path on success
 * @throws MountFailure carrying the typed `MountError`; `asMountError` gets it back, and
 *   `renderMountError` words it
 */
export async function mountNetworkShare(
  server: string,
  share: string,
  username: string | null,
  password: string | null,
  port?: number,
  timeoutMs?: number,
  hostName?: string,
  attemptId?: string,
): Promise<MountResult> {
  const res = await commands.mountNetworkShare(
    server,
    share,
    username,
    password,
    port ?? null,
    timeoutMs ?? null,
    hostName ?? null,
    attemptId ?? null,
  )
  if (res.status === 'error') throwMountError(res.error)
  return res.data
}

/** Where a "Connect directly" left the volume. Every outcome is an answer, never a throw. */
export type { UpgradeResult }

/**
 * Upgrades an existing OS-mounted SMB volume to use a direct smb2 connection.
 *
 * Tries stored credentials first. Answers `credentialsNeeded` when the frontend
 * should ask for a password, `networkError` for a server that didn't cooperate,
 * and `volumeGone` / `notSmbMount` when there's no OS-mounted share to upgrade.
 */
export async function upgradeToSmbVolume(volumeId: string): Promise<UpgradeResult> {
  return commands.upgradeToSmbVolume(volumeId)
}

/**
 * Whether the system (login) keychain holds a password another app (Finder) saved for
 * this volume's SMB server. Attribute-only probe — never triggers the macOS consent
 * dialog — so the UI can decide whether to offer "use the saved password". Best-effort:
 * any error resolves to `false` (the offer simply doesn't appear). macOS only; `false`
 * elsewhere.
 */
export async function systemHasSavedSmbPassword(volumeId: string): Promise<boolean> {
  const res = await commands.systemHasSavedSmbPassword(volumeId)
  return res.status === 'ok' ? res.data : false
}

/**
 * Upgrades an OS-mounted SMB volume to direct smb2 using the password Finder already
 * saved in the login keychain. Reading it triggers the macOS consent dialog (prime the
 * user first). On success the password is copied into Cmdr's own store so future
 * reconnects are silent; if nothing is saved or the user denies, the result is
 * `credentialsNeeded` so the caller falls back to the login form.
 */
export async function upgradeToSmbVolumeUsingSavedPassword(volumeId: string): Promise<UpgradeResult> {
  return commands.upgradeToSmbVolumeUsingSavedPassword(volumeId)
}

/**
 * The account macOS saved a password under for `host`'s server, and whether that password may
 * still be tried this run. An attributes-only read: ❌ never a consent dialog, so the sheet may
 * ask it whenever it opens. `null` off macOS, when nothing is saved, or when the read broke down.
 */
export async function systemSavedSmbAccount(host: NetworkHost): Promise<SystemSavedAccount | null> {
  try {
    return await commands.systemSavedSmbAccount(wireHost(host))
  } catch {
    return null
  }
}

/**
 * Lists `host`'s shares with the password macOS saved for its server. ❗ Raises the macOS
 * consent dialog, so call it only on a person's action: opening a share list that needs a
 * sign-in, or "Use the password macOS saved". The backend asks at most once per server per run.
 * @returns the listing, or `null` when there was nothing to try (none saved, denied, or asked already)
 * @throws ShareListFailure carrying the typed `ShareListError`; `shareListErrorOf` gets it back
 */
export async function listSharesWithSystemSavedPassword(host: NetworkHost): Promise<ShareListResult | null> {
  const res = await commands.listSharesWithSystemSavedPassword(wireHost(host))
  if (res.status === 'error') throwShareListError(res.error)
  return res.data
}

/** What switching a share's direct connection did. */
export type { DirectConnectionSwitch }

/**
 * Whether the SMB share behind `volumeId` may use Cmdr's fast direct connection (the
 * per-share switch), or `null` when there's no SMB share behind it to ask about.
 */
export async function getSmbDirectConnectionEnabled(volumeId: string): Promise<boolean | null> {
  return commands.getSmbDirectConnectionEnabled(volumeId)
}

/**
 * Switches the SMB share behind `volumeId` onto or off Cmdr's fast direct connection.
 * Off on a direct share hands it back to the macOS mount right away
 * (`returnedToOsMount`); on only saves, and the caller runs "Connect directly".
 */
export async function setSmbDirectConnectionEnabled(
  volumeId: string,
  enabled: boolean,
): Promise<DirectConnectionSwitch> {
  return commands.setSmbDirectConnectionEnabled(volumeId, enabled)
}

/** Where `..` in an SMB share's top folder leads, and the share's own switch. */
export type ShareParent = Omit<ShareParentWire, 'host'> & { host: NetworkHost }

/**
 * Where `..` in the top folder of the SMB share behind `volumeId` leads (its server, and
 * what to call it), plus the share's own "Show “..”" choice (`null` while it follows the
 * default). `null` when no SMB share is behind it.
 */
export async function getSmbShareParent(volumeId: string): Promise<ShareParent | null> {
  const parent = await commands.getSmbShareParent(volumeId)
  return parent ? shareParentFromWire(parent) : null
}

/**
 * The frontend's `ShareParent` from the wire's, wherever one arrives (the command above, a
 * top-folder listing's `listing-complete`). The wire spells an unresolved name `null`; the
 * frontend's `NetworkHost` leaves it out.
 */
export function shareParentFromWire(parent: ShareParentWire): ShareParent {
  const { hostname, ipAddress, ...host } = parent.host
  return {
    ...parent,
    host: { ...host, hostname: hostname ?? undefined, ipAddress: ipAddress ?? undefined },
  }
}

/**
 * Saves the share's "Show “..”" choice against `defaultShow` (the setting): a choice that
 * matches the default clears the share's override. `false` when no SMB share is behind
 * `volumeId`, and nothing was saved.
 */
export async function setSmbShareParentRow(volumeId: string, show: boolean, defaultShow: boolean): Promise<boolean> {
  return commands.setSmbShareParentRow(volumeId, show, defaultShow)
}

/**
 * Upgrades an SMB volume using explicit credentials from the login form.
 */
export async function upgradeToSmbVolumeWithCredentials(
  volumeId: string,
  username: string | null,
  password: string | null,
  rememberInKeychain: boolean,
): Promise<UpgradeResult> {
  return commands.upgradeToSmbVolumeWithCredentials(volumeId, username, password, rememberInKeychain)
}

/**
 * Tries to rebuild a Disconnected volume's session in place.
 *
 * Backend-neutral: every remote backend implements this, and the per-volume
 * reconnect manager drives SMB, SFTP, and WebDAV through it on each backoff tick
 * (and on "Retry now" / lazy nav-time retry). The backend single-flights
 * concurrent calls. Resolves on success; a refusal throws a `ReconnectFailure`
 * carrying the typed `ReconnectError`, which `asReconnectError` gets back.
 */
export async function reconnectVolume(volumeId: string): Promise<void> {
  const res = await commands.reconnectVolume(volumeId)
  if (res.status === 'error') throwReconnectError(res.error)
}

/**
 * Reconnects a volume with freshly-entered credentials. Used by the "Sign in"
 * affordance shown when an in-place reconnect gave up on an auth failure (the
 * saved password went stale). The backend reconnects and, only once that
 * connected, files the credentials the way `remember` (the Remember box) asks:
 * on saves them, off only refreshes an entry still there. On success a
 * `volume-connection-changed { state: "connected" }` event follows.
 *
 * Whether the USERNAME may change is the backend's call, not this wrapper's:
 * `getVolumeSignInState` is what tells the sheet which shape to render.
 *
 * Resolves on success with `rememberRefused` set when the box was on and the
 * Keychain wouldn't keep the password (the connection stands either way); a
 * refusal throws a `ReconnectFailure` carrying the typed `ReconnectError`, which
 * `asReconnectError` gets back.
 */
export async function reconnectVolumeWithCredentials(
  volumeId: string,
  username: string,
  password: string,
  remember: boolean,
): Promise<SignedInReply> {
  const res = await commands.reconnectVolumeWithCredentials(volumeId, username, password, remember)
  if (res.status === 'error') throwReconnectError(res.error)
  return res.data
}

/**
 * Files the credentials of a sign-in that ALREADY connected this volume, without
 * dialing: the sheet's "Try again" after `rememberRefused` (#603). A second
 * `reconnectVolumeWithCredentials` would find the session live and save nothing.
 *
 * ❗ Only for the secret that round connected with. Same reply and refusals as
 * `reconnectVolumeWithCredentials`.
 */
export async function rememberVolumeSignIn(
  volumeId: string,
  username: string,
  password: string,
  remember: boolean,
): Promise<SignedInReply> {
  const res = await commands.rememberVolumeSignIn(volumeId, username, password, remember)
  if (res.status === 'error') throwReconnectError(res.error)
  return res.data
}

/**
 * What a "Sign in" affordance on this volume may ask the user for, right now.
 *
 * Ask when the affordance renders, and never hold on to the answer: a backend
 * that authenticates per connection can prove itself with a different credential
 * each time it dials, so a value kept from earlier describes a session that may
 * already be gone.
 *
 * Backend-neutral. A volume with no sign-in story of its own, and an id nothing
 * is registered under, both answer `'password'`, which is the safe way to be
 * wrong: a needless password box is recoverable, a wrong `'nothing'` is a volume
 * the user can't sign in to at all.
 */
export async function getVolumeSignInState(volumeId: string): Promise<SignInShape> {
  return await commands.getVolumeSignInState(volumeId)
}

/**
 * Disconnects an SMB volume by unmounting it at the OS level (macOS) or by
 * dropping the smb2 session (Linux, until GVFS unmount is wired up). The
 * `volumes-changed` event removes the volume from the picker shortly after.
 *
 * Resolves on success; a refusal throws an `EjectFailure` carrying the typed
 * `EjectError` (`unmountRefused` when a Finder window still has the share
 * open). `asEjectError` gets it back, and `renderEjectError` words it.
 */
export async function disconnectSmbVolume(volumeId: string): Promise<void> {
  const res = await commands.disconnectSmbVolume(volumeId)
  if (res.status === 'error') throwEjectError(res.error)
}

// ============================================================================
// Manual server management (macOS only)
// ============================================================================

/**
 * Connects to a manually-specified server: parses address, checks TCP reachability,
 * persists the entry, and injects a synthetic host into the discovery state.
 * @param address Hostname, IP, IP:port, or smb:// URL
 * @param name What the Add form's Name field held; empty leaves the server unnamed
 * @param username The account the person means to sign in as, or `null` for none
 * @param checkReachability `false` for the sheet's "Add anyway": saves without probing
 * @returns The injected host and optional share path
 * @throws AddServerFailure carrying the typed `AddServerError` (`asAddServerError`)
 */
export async function connectToServer(
  address: string,
  name = '',
  username: string | null = null,
  checkReachability = true,
): Promise<ManualConnectResult> {
  const res = await commands.connectToServer(address, name === '' ? null : name, username, checkReachability)
  if (res.status === 'error') throwAddServerError(res.error)
  return res.data as ManualConnectResult
}

/**
 * Removes a manually-added server by ID.
 * Deletes from persistent storage and removes from discovery state.
 * @param serverId The manual server's host ID (like "manual-192-168-1-100-445")
 */
/**
 * Sets the account the SMB server `host` is used with, the same preference a typed
 * username is: "Sign in as…" in its share list, or `null` for guest. Answers whether the
 * store could be written. ❗ The whole host, ❌ never its name: the backend saves it at the
 * address the host is dialed at, and a Bonjour name (`SMB Test (Auth)`) is no address.
 */
export async function setSmbAccountPreference(host: NetworkHost, username: string | null): Promise<boolean> {
  return await commands.setSmbAccountPreference(wireHost(host), username)
}

/**
 * Tells the backend the user took a network action: opening the Servers view,
 * "Connect to server…", or upgrading a mounted share to direct smb2. It brings back
 * the manual servers and runs the existing-SMB-mount upgrade pass. It starts no browse
 * of its own: see `setServersViewShown`.
 */
export async function noteNetworkAction(): Promise<void> {
  try {
    await commands.noteNetworkAction()
  } catch {
    // Stub on unsupported platforms. Silently swallow.
  }
}

/**
 * Whether any Servers view is on screen. The backend browses mDNS only while one is
 * (or while something else needs discovery), so hosts arrive and leave live there and
 * nothing browses at idle. The first browse is what raises macOS's "Cmdr wants to
 * find devices on local networks" prompt; the backend won't browse while
 * `network.enabled` is off.
 */
export async function setServersViewShown(shown: boolean): Promise<void> {
  try {
    await commands.setServersViewShown(shown)
  } catch {
    // Stub on unsupported platforms. Silently swallow.
  }
}

/**
 * Pushes the `network.enabled` toggle live to the backend. When `false`, stops mDNS and
 * clears the discovered host list (the frontend store empties via `network-host-lost`).
 * When `true`, the browse resumes if a Servers view is holding it.
 */
export async function setNetworkEnabled(enabled: boolean): Promise<void> {
  try {
    await commands.setNetworkEnabled(enabled)
  } catch {
    // Stub on unsupported platforms. Silently swallow.
  }
}

// ============================================================================
// Network host context menu
// ============================================================================

/**
 * Subscribes to the notice that a share Cmdr tried to take over is staying on the
 * macOS kernel mount (slower, and outside Cmdr's control).
 *
 * Fires at most once per server per app run: the backend's notice ledger collapses
 * the one-call-per-mounted-share fallback into one message. Call the returned
 * `UnlistenFn` on destroy.
 */
export function onSmbFellBackToOsMount(handler: (payload: SmbFellBackToOsMount) => void): Promise<UnlistenFn> {
  return events.smbFellBackToOsMount.listen((event) => {
    handler(event.payload)
  })
}

/**
 * Subscribes to the backend taking back a slow-connection notice it raised with
 * `smb-fell-back-to-os-mount`: the share's direct connection was switched off, so
 * the notice's retry has nothing left to offer. Call the returned `UnlistenFn` on
 * destroy.
 */
export function onSmbOsMountNoticeWithdrawn(
  handler: (payload: SmbOsMountNoticeWithdrawn) => void,
): Promise<UnlistenFn> {
  return events.smbOsMountNoticeWithdrawn.listen((event) => {
    handler(event.payload)
  })
}

/** A `NetworkHost` as the IPC commands take it. */
function wireHost(host: NetworkHost) {
  return {
    id: host.id,
    name: host.name,
    hostname: host.hostname ?? null,
    ipAddress: host.ipAddress ?? null,
    port: host.port,
    source: host.source,
  }
}

/**
 * Unmounts all SMB shares mounted from `host`, answering the mount paths that went.
 * ❗ The whole host: its mounts are found by where it dials (hostname, IP, each on
 * its port), never by a display name. The backend never refuses: a host with nothing
 * mounted answers an empty list.
 */
export async function disconnectNetworkHost(host: NetworkHost): Promise<string[]> {
  return commands.disconnectNetworkHost(wireHost(host))
}

/**
 * Whether a share from `host` is mounted, found the way `disconnectNetworkHost` finds what
 * to unmount: the kernel's mount table, matched by where the host dials. What a row menu's
 * Disconnect asks before it's offered. Reads no password.
 */
export async function smbHostHasMounts(host: NetworkHost): Promise<boolean> {
  return commands.smbHostHasMounts(wireHost(host))
}
