/**
 * Signing in to LIST a server's shares: the sheet round, the listing it runs, and every
 * side effect a listing that worked leaves behind.
 *
 * The share list's own component only renders what these answer. Each function takes the
 * server (its `NetworkHost` and the label the person knows it by) and answers an outcome;
 * ❌ none of them touches a view. Where cancelling lands is the CALLER's call, because it
 * differs per site: a refused listing stays on its in-pane Sign in, "Sign in as…" on the list. The rule
 * every door follows: a sheet opened over an in-pane sign-in state leaves the pane on it, focus on its
 * Sign in (a refused mount's too: `../pane/share-mount.svelte.ts`).
 *
 * ❗ **One ask at a time, guarded HERE** (`sheetRound`), since three doors (a Servers
 * row's menu, the path bar's account, the palette) and the share list itself all open the
 * sheet: a second `openSignInSheet` closes the first as `cancelled`, and a caller reading
 * that as "the person said no" walks away from under a sheet that's still up. A round asked for
 * while one is up answers `null` and asks nothing.
 *
 * Every door to an account action runs `runServerAccountAction`, so the three can't drift
 * on what an action does or how it says it went.
 *
 * Mount credentials are the mount's question, ❌ not this module's: `../pane/share-mount.svelte.ts`.
 */

import type { NetworkHost, ShareListError, ShareListResult } from '../types'
import { shareListErrorOf } from './share-list-error'
import {
  forgetCredentials,
  getListedAccount,
  guestListingWorked,
  setCredentialStatus,
  setListedAccount,
} from './network-store.svelte'
import { renderShareListError } from './share-list-error-messages'
import { resolveShareListServer } from './share-list-label'
import type { ServerAccountAction, ServerAccountFacts } from '../navigation/row-menu'
import {
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

/** What one listing round with an account (or as guest) came to. */
export type ShareListAttempt =
  /**
   * It listed. ❗ No password rides back: the backend keeps the account as the server's
   * session credential, which the share list's re-list and a mount sign in with.
   */
  | { kind: 'listed'; result: ShareListResult }
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
 * On success it leaves the server's whole record behind: the backend's share cache the
 * pane's re-list reads, the credential status, the listed account the header names, the
 * account preference (❗ the choice sticks, so later listings sign in as it instead of as
 * guest), the Keychain entry when `remember`, and the known-shares store.
 */
export async function listWithAccount(
  server: ShareListServer,
  answer: SmbCredentialAnswer,
  credentialSource: 'typed' | 'saved',
): Promise<ShareListAttempt> {
  const { host } = server
  const { username, password, remember } = answer
  try {
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

    setCredentialStatus(host.name, username ? 'has_creds' : 'no_creds')
    setListedAccount(host.id, username !== null ? { kind: 'user', username } : { kind: 'guest' })
    void setSmbAccountPreference(host, username).catch(() => {})

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
    return { kind: 'listed', result }
  } catch (e) {
    const error = shareListErrorOf(e)
    if (error.type === 'auth_failed') setCredentialStatus(host.name, 'failed')
    const refusal = refusalForShareError(error)
    if (refusal === 'authentication_rejected' || refusal === 'needs_credentials') return { kind: 'refused', refusal }
    return { kind: 'failed', error }
  }
}

/** Whether a listing's sign-in sheet is up (`sheetRound`), so a list stops offering rows behind it. */
let sheetUp = false

/** Whether a share list's sign-in sheet is up right now. Read when asked, ❌ not reactive. */
export function isShareListSheetUp(): boolean {
  return sheetUp
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
): Promise<ShareListSheetOutcome | null> {
  if (sheetUp) return null
  sheetUp = true
  try {
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
  } finally {
    sheetUp = false
  }
}

/**
 * The LISTING needs a sign-in: the sheet, opening with the refusal that asked.
 * `guestAllowed` per the listing's `authMode`. `null` when another round's sheet is up.
 */
export async function askToList(
  server: ShareListServer,
  options: { refusal: ConnectRefusalKind; guestAllowed: boolean },
): Promise<ShareListSheetOutcome | null> {
  return await sheetRound(server, options)
}

/**
 * The sign-in a server's share list asks for when its listing came back refused (the
 * error screen's Sign in, and the ask a listing the person started raises on its own):
 * the sheet, opening with the refusal that asked. `path` is the share list's, which names
 * the server. No guest choice: the listing already tried guest where it could
 * (`network/share_list_sign_in.rs`), and that's what came back refused. `null` when the
 * path names no server this session knows, or another round's sheet is up.
 *
 * ❗ A server no discovered host stands for (one saved without contact) signs in to the
 * host the backend's own listing resolved (`resolveShareListServer`).
 */
export async function askToSignInToList(path: string, refusal: ShareListError): Promise<ShareListSheetOutcome | null> {
  const server = await resolveShareListServer(path)
  if (!server) return null
  return await askToList(server, { refusal: refusalForShareError(refusal), guestAllowed: false })
}

/**
 * "Sign in as…": the same sheet, asked by the person rather than a refusal, so it opens
 * with no refusal sentence, prefilled with the account the list is signed in as. No guest
 * choice: the sheet selects guest whenever it offers it, and this is for an account.
 * `null` when another round's sheet is up.
 */
export async function signInAs(
  server: ShareListServer,
  listedAccount: SignedInAs | undefined,
): Promise<ShareListSheetOutcome | null> {
  return await sheetRound(server, {
    guestAllowed: false,
    initialUsername: listedAccount?.kind === 'user' ? listedAccount.username : undefined,
  })
}

/** "Browse as guest": the way back from an account, where a guest listing worked on this server. */
export async function useGuest(server: ShareListServer): Promise<ShareListAttempt> {
  // ❗ The account goes FIRST: while it's set, the backend never lists this server as guest.
  await setSmbAccountPreference(server.host, null).catch(() => false)
  return await listWithAccount(server, { username: null, password: null, remember: false }, 'typed')
}

/**
 * "Forget saved password", with the toast that says how it went. Answers whether none is
 * stored now: forgotten, or there was none to forget.
 *
 * ❗ Offered unconditionally, ❌ never gated on "is one stored?", which costs a Keychain read
 * (`../navigation/row-menu.ts`). So this is where "was there one?" gets answered: the store's
 * typed `not_found` says no, and the person hears so.
 */
export async function forgetPassword(server: ShareListServer): Promise<boolean> {
  try {
    await forgetCredentials(server.host.name)
    addToast(tString('fileExplorer.network.forgotPassword', { hostName: server.label }), { level: 'success' })
    return true
  } catch (e) {
    if (isNothingStored(e)) {
      setCredentialStatus(server.host.name, 'no_creds')
      addToast(tString('fileExplorer.navigation.forgetSecretNoneToast', { name: server.label }), { level: 'info' })
      return true
    }
    addToast(tString('fileExplorer.network.deletePasswordFailed'), { level: 'error' })
    return false
  }
}

/** The credential store's typed "nothing under that name" (`KeychainError::NotFound`, its `type` tag). */
function isNothingStored(e: unknown): boolean {
  return typeof e === 'object' && e !== null && (e as { type?: unknown }).type === 'not_found'
}

/**
 * What the account doors read to decide their items, off what this session already knows
 * (the listings' own answers), ❌ never a Keychain read.
 */
export function serverAccountFacts(host: NetworkHost): ServerAccountFacts {
  return { listedAccount: getListedAccount(host.id), guestListingWorked: guestListingWorked(host.id) }
}

/**
 * Runs an account action picked at any door: a Servers row's menu, the path bar's account
 * menu, or the palette. Answers whether a listing landed, so a door that isn't on the share
 * list (a Servers row) opens it, the way activating the row does. A refusal about the server is
 * toasted in the share list's own words; a cancelled sheet, or one already up, says nothing.
 *
 * ❗ "Browse as guest" runs only where a guest listing worked: it drops the account FIRST, so
 * a server that turns the guest away would leave the person with neither. Every door hides it
 * then (`row-menu.ts::serverAccountEntries`); a rebound palette key gets a sentence.
 */
export async function runServerAccountAction(
  action: ServerAccountAction,
  server: ShareListServer,
): Promise<{ listed: boolean }> {
  const facts = serverAccountFacts(server.host)
  switch (action) {
    case 'sign-in-as':
      return answered(server, await signInAs(server, facts.listedAccount))
    case 'browse-as-guest':
      if (!facts.guestListingWorked) {
        addToast(tString('servers.hub.browseAsGuestUnavailable', { name: server.label }), { level: 'info' })
        return { listed: false }
      }
      return answered(server, await useGuest(server))
    case 'forget-secret':
      await forgetPassword(server)
      return { listed: false }
  }
}

/** What a door says about an account action's listing. A refused guest is a server that wants an account. */
function answered(
  server: ShareListServer,
  attempt: ShareListAttempt | ShareListSheetOutcome | null,
): { listed: boolean } {
  if (attempt?.kind === 'failed') {
    addToast(renderShareListError(attempt.error, server.label), { level: 'error' })
  } else if (attempt?.kind === 'refused') {
    addToast(tString('servers.hub.browseAsGuestUnavailable', { name: server.label }), { level: 'info' })
  }
  return { listed: attempt?.kind === 'listed' }
}
