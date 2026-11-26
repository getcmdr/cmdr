/**
 * Signing in to LIST a server's shares: the sheet round, the listing it runs, and every
 * side effect a listing that worked leaves behind.
 *
 * The share list's own component only renders what these answer. Each function takes the
 * server (its `NetworkHost` and the label the person knows it by) and answers an outcome;
 * ❌ none of them touches a view. Where cancelling lands is the CALLER's call, because it
 * differs per site: a refused listing goes back to the Servers list, "Sign in as…" stays.
 *
 * ❗ **One ask at a time is the caller's guard.** A second `openSignInSheet` closes the first
 * as `cancelled`, and a caller reading that as "the person said no" walks away from under a
 * sheet that's still up. The share list holds that flag anyway (it stops offering rows
 * while the sheet is up), so it's the one that checks it.
 *
 * Mount credentials are the mount's question, ❌ not this module's: `../pane/share-mount.svelte.ts`.
 */

import type { NetworkHost, ShareListError, ShareListResult } from '../types'
import { shareListErrorOf } from './share-list-error'
import {
  clearShareState,
  forgetCredentials,
  setCredentialStatus,
  setListedAccount,
  setShareState,
} from './network-store.svelte'
import {
  getSmbCredentials,
  isUsingCredentialFileFallback,
  listSharesWithCredentials,
  saveSmbCredentials,
  setSmbAccountPreference,
  updateKnownShare,
} from '#lib/tauri-commands/index.js'
import { addToast } from '#lib/ui/toast/index.js'
import { tString } from '#lib/intl/messages.svelte.js'
import { getNetworkTimeoutMs, getShareCacheTtlMs } from '#lib/settings/network-settings.js'
import { openSmbSignInSheet, refusalForShareError, type SmbCredentialAnswer } from './smb-sign-in'
import type { ConnectRefusalKind } from '#lib/servers/connect-refusals.js'
import type { SignedInAs } from './signed-in-as'

/** The server whose shares are being listed. */
export interface ShareListServer {
  /** ❗ `host.name` is the key every credential lookup uses; `host.id` the share cache's. */
  host: NetworkHost
  /** What to call it in the sheet and the toasts: the name the person gave it, else `host.name`. */
  label: string
}

/** An account that listed the server, kept for the mount that follows (an empty password is valid SMB). */
export interface ShareListCredentials {
  username: string
  password: string
}

/** What one listing round with an account (or as guest) came to. */
export type ShareListAttempt =
  /** It listed. `credentials` is the account it listed as, `null` for guest. */
  | { kind: 'listed'; result: ShareListResult; credentials: ShareListCredentials | null }
  /** The server turned the CREDENTIAL away: the sheet stays up for another try. */
  | { kind: 'refused'; refusal: 'authentication_rejected' | 'needs_credentials' }
  /** Anything about the SERVER: the share list's own error state words it, with its retry. */
  | { kind: 'failed'; error: ShareListError }

/**
 * What a sheet round came to: the attempt that closed it, or the person cancelling.
 * ❗ A refused attempt never closes the sheet, so it's never the answer here.
 */
export type ShareListSheetOutcome = { kind: 'cancelled' } | Exclude<ShareListAttempt, { kind: 'refused' }>

async function notifyIfUsingFileFallback(): Promise<void> {
  if (await isUsingCredentialFileFallback()) {
    addToast(tString('fileExplorer.network.share.credentialsStoredLocally'), {
      level: 'info',
      id: 'credential-file-fallback',
    })
  }
}

/**
 * One listing round-trip with what the person offered (`username: null` is guest).
 *
 * On success it leaves the server's whole record behind: the share cache the hub reads,
 * the credential status, the listed account the header names, the account preference
 * (❗ the choice sticks, so later listings, the background ones included, sign in as it
 * instead of as guest), the Keychain entry when `remember`, and the known-shares store.
 */
export async function listWithAccount(
  server: ShareListServer,
  answer: SmbCredentialAnswer,
  credentialSource: 'typed' | 'saved',
): Promise<ShareListAttempt> {
  const { host } = server
  const { username, password, remember } = answer
  try {
    // A fresh listing, ❌ never the cached one.
    clearShareState(host.id)

    const result = await listSharesWithCredentials(
      host.id,
      host.hostname ?? host.name,
      host.ipAddress,
      host.port,
      username,
      password,
      credentialSource,
      getNetworkTimeoutMs(),
      getShareCacheTtlMs(),
    )

    // The hub reads the same server's state.
    setShareState(host.id, result)
    setCredentialStatus(host.name, username ? 'has_creds' : 'no_creds')
    setListedAccount(host.id, username !== null ? { kind: 'user', username } : { kind: 'guest' })
    void setSmbAccountPreference(host.name, username).catch(() => {})

    // ❗ Written only once the account has worked.
    if (remember && username !== null && password !== null) {
      await saveSmbCredentials(host.name, null, username, password)
      await notifyIfUsingFileFallback()
    }

    await updateKnownShare(
      host.name,
      '', // Server-level, not share-specific
      username ? 'credentials' : 'guest',
      result.authMode === 'guest_allowed' ? 'guest_or_credentials' : 'credentials_only',
      username,
    )
    return {
      kind: 'listed',
      result,
      credentials: username !== null ? { username, password: password ?? '' } : null,
    }
  } catch (e) {
    const error = shareListErrorOf(e)
    if (error.type === 'auth_failed') setCredentialStatus(host.name, 'failed')
    const refusal = refusalForShareError(error)
    if (refusal === 'authentication_rejected' || refusal === 'needs_credentials') return { kind: 'refused', refusal }
    return { kind: 'failed', error }
  }
}

/**
 * Cmdr's stored password for the server, or `null` when none is saved.
 *
 * ❗ ONE Keychain read, ❌ never preceded by a "has one?" check: each access can raise a
 * system prompt, and asking whether one exists costs the same prompt as reading it.
 */
export async function readStoredCredentials(server: ShareListServer): Promise<ShareListCredentials | null> {
  try {
    const creds = await getSmbCredentials(server.host.name, null)
    return { username: creds.username, password: creds.password }
  } catch {
    return null
  }
}

/**
 * The silent rung before the sheet: list with the stored password, if one is saved.
 * Answers `null` when nothing is stored; otherwise the credentials it read (the mount
 * reuses them whatever the listing said) and what the listing came to.
 */
export async function tryStoredCredentials(
  server: ShareListServer,
): Promise<{ credentials: ShareListCredentials; attempt: ShareListAttempt } | null> {
  const credentials = await readStoredCredentials(server)
  if (!credentials) return null
  const attempt = await listWithAccount(
    server,
    { username: credentials.username, password: credentials.password, remember: false },
    'saved',
  )
  return { credentials, attempt }
}

/**
 * Runs one sheet round whose every attempt is a listing, and answers the attempt that
 * closed it. A refused attempt keeps the sheet open; a listing that worked or a failure
 * about the server closes it (`handed_off`: nothing here connects a VOLUME, so the list
 * it just loaded is the result).
 */
async function sheetRound(
  server: ShareListServer,
  options: { guestAllowed: boolean; refusal?: ConnectRefusalKind; initialUsername?: string },
): Promise<ShareListSheetOutcome> {
  let closing: ShareListSheetOutcome = { kind: 'cancelled' }
  const result = await openSmbSignInSheet({
    host: server.host,
    label: server.label,
    guestAllowed: options.guestAllowed,
    refusal: options.refusal,
    initialUsername: options.initialUsername,
    attempt: async (answer) => {
      const attempt = await listWithAccount(server, answer, 'typed')
      if (attempt.kind === 'refused') return attempt
      closing = attempt
      return { kind: 'handed_off' }
    },
  })
  return result.kind === 'cancelled' ? { kind: 'cancelled' } : closing
}

/**
 * The LISTING needs a sign-in: the sheet, opening with the refusal that asked.
 * `guestAllowed` per the listing's `authMode`.
 */
export async function askToList(
  server: ShareListServer,
  options: { refusal: ConnectRefusalKind; guestAllowed: boolean },
): Promise<ShareListSheetOutcome> {
  return await sheetRound(server, options)
}

/**
 * "Sign in as…": the same sheet, asked by the person rather than a refusal, so it opens
 * with no refusal sentence, prefilled with the account the list is signed in as. No guest
 * choice: the sheet selects guest whenever it offers it, and this is for an account.
 */
export async function signInAs(
  server: ShareListServer,
  listedAccount: SignedInAs | undefined,
): Promise<ShareListSheetOutcome> {
  return await sheetRound(server, {
    guestAllowed: false,
    initialUsername: listedAccount?.kind === 'user' ? listedAccount.username : undefined,
  })
}

/** "Browse as guest": the way back from an account, where a guest listing worked on this server. */
export async function useGuest(server: ShareListServer): Promise<ShareListAttempt> {
  // ❗ The account goes FIRST: while it's set, the backend never lists this server as guest.
  await setSmbAccountPreference(server.host.name, null).catch(() => false)
  return await listWithAccount(server, { username: null, password: null, remember: false }, 'typed')
}

/** "Forget saved password", with the toast that says how it went. Answers whether it went through. */
export async function forgetPassword(server: ShareListServer): Promise<boolean> {
  try {
    await forgetCredentials(server.host.name)
    addToast(tString('fileExplorer.network.forgotPassword', { hostName: server.label }), { level: 'success' })
    return true
  } catch {
    addToast(tString('fileExplorer.network.deletePasswordFailed'), { level: 'error' })
    return false
  }
}
