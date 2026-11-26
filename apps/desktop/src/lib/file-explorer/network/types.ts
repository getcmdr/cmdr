/**
 * The network vocabulary: discovered hosts, SMB shares, known shares, and why a
 * mount didn't go through. Re-exported from `#lib/file-explorer/types`, which
 * stays the one import seam for consumers.
 */

// ============================================================================
// Network discovery types
// ============================================================================

/** State of network host discovery. */
export type DiscoveryState = 'idle' | 'searching' | 'active'

/** A discovered network host advertising SMB services. */
export interface NetworkHost {
  /** Unique identifier for the host (derived from service name) */
  id: string
  /** Display name (the advertised service name) */
  name: string
  /** Resolved hostname (like "macbook.local"), or undefined if not yet resolved */
  hostname?: string
  /** Resolved IP address, or undefined if not yet resolved */
  ipAddress?: string
  /** SMB port (usually 445) */
  port: number
  /** How this host was added: mDNS discovery or manual user entry */
  source?: 'discovered' | 'manual'
}

// ============================================================================
// SMB share types
// ============================================================================

/** Information about a discovered SMB share. */
export interface ShareInfo {
  /** Name of the share (for example, "Documents", "Media") */
  name: string
  /** Whether this is a disk share (true) or other type like printer/IPC */
  isDisk: boolean
  /** Optional description/comment for the share */
  comment: string | null
}

/** Authentication mode detected for a host. */
export type AuthMode = 'guest_allowed' | 'creds_required' | 'unknown'

/** Result of a share listing operation. */
export interface ShareListResult {
  /** Shares found on the host (already filtered to disk shares only) */
  shares: ShareInfo[]
  /** Authentication mode detected */
  authMode: AuthMode
  /** Whether this result came from cache */
  fromCache: boolean
}

/**
 * Why a server's share list didn't load. `message` is diagnostic detail for the
 * log; `share-list-error-messages.ts` words the type. Re-exported from the
 * generated bindings rather than restated, so it can't drift from the Rust enum.
 */
export type { ShareListError } from '#lib/ipc/bindings.js'

// ============================================================================
// Known shares store types
// ============================================================================

/** Connection mode used for the last successful connection. */
export type ConnectionMode = 'guest' | 'credentials'

/** Authentication options available for a share. */
export type AuthOptions = 'guest_only' | 'credentials_only' | 'guest_or_credentials'

/** How a share row's server GUID was learned: only a GUID a sign-in recorded lets one share answer for another. */
export type GuidProof = 'authenticated' | 'unauthenticated'

/** Information about a known network share (previously connected). */
export interface KnownNetworkShare {
  /** Hostname or IP of the server */
  serverName: string
  /** Name of the specific share */
  shareName: string
  /** Protocol type (currently only "smb") */
  protocol: string
  /** When we last successfully connected (ISO 8601) */
  lastConnectedAt: string
  /** How we connected last time */
  lastConnectionMode: ConnectionMode
  /** Auth options detected last time */
  lastKnownAuthOptions: AuthOptions
  /** Username used (null for guest) */
  username: string | null
  // Share rows only, what their last mount had (`docs/specs/saved-smb-shares.md`); `port` is null for 445.
  address?: string | null
  port?: number | null
  volumeId?: string | null
  mountPath?: string | null
  pinned?: boolean
  /** Share rows only: the GUID the share's server answered NEGOTIATE with, which proves two rows one share. */
  serverGuid?: string | null
  /** How `serverGuid` was learned. */
  serverGuidProof?: GuidProof
}

// ============================================================================
// Mount types
// ============================================================================

/**
 * Why a mount didn't go through, as typed data (`mount-error-messages.ts` words it).
 * Re-exported from the generated bindings rather than restated, so it can't drift
 * from the Rust enum.
 */
export type { MountError } from '#lib/ipc/bindings.js'
