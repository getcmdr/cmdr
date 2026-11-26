import type { ListingErrorReason } from '#lib/ipc/bindings.js'

/**
 * Reasons that say the path IS there and this account, app, or device may not read it:
 * the OS, macOS privacy (TCC), a file server, an object store, or a phone refused it. A
 * server's share list the server refused (or couldn't be asked) counts too: the path names
 * the server itself, and the existence probe would only ask the share cache, whose "no"
 * walked the pane up to the Servers hub, which lists nothing.
 */
const ACCESS_REFUSALS: ReadonlySet<ListingErrorReason['reason']> = new Set([
  'permissionDenied',
  'remotePermissionDenied',
  'noPermissionErrno',
  'notPermitted',
  'tccRestricted',
  'objectStoreRefused',
  'serverRefused',
  'phoneRefused',
  'shareListSignIn',
  'shareListUnavailable',
])

/**
 * Whether a listing failed because access was refused. A refusal is final: the pane shows
 * the error where it is, ❌ never an existence probe, which some backends can't answer for
 * a folder they may not read and which reads "no" as "deleted" (#1018).
 */
export function isAccessRefusal(reason: ListingErrorReason | null | undefined): boolean {
  return reason != null && ACCESS_REFUSALS.has(reason.reason)
}
