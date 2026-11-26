/**
 * Tests for `share-list-sign-in.ts`, signing in to LIST a server's shares.
 *
 * ❗ The sign-in is the one sheet, so these assert the REQUEST each round makes (its
 * shape, its endpoint, its refusal) and what its `attempt` runs, ❌ never a form.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { NetworkHost, ShareInfo, ShareListResult } from '../types'
import { ShareListFailure } from './share-list-error'
import {
  askToList,
  forgetPassword,
  isShareListSheetUp,
  listWithAccount,
  readStoredCredentials,
  runServerAccountAction,
  signInAs,
  tryStoredCredentials,
  useGuest,
  type ShareListServer,
} from './share-list-sign-in'

const h = vi.hoisted(() => ({
  listSharesWithCredentials: vi.fn(),
  getSmbCredentials: vi.fn(),
  saveSmbCredentials: vi.fn(() => Promise.resolve()),
  setSmbAccountPreference: vi.fn(() => Promise.resolve(true)),
  updateKnownShare: vi.fn(() => Promise.resolve()),
  isUsingCredentialFileFallback: vi.fn(() => Promise.resolve(false)),
  openSignInSheet: vi.fn(),
  clearShareState: vi.fn(),
  setShareState: vi.fn(),
  setCredentialStatus: vi.fn(),
  setListedAccount: vi.fn(),
  forgetCredentials: vi.fn(() => Promise.resolve()),
  addToast: vi.fn(() => 'id'),
  getListedAccount: vi.fn((_hostId: string): unknown => undefined),
  guestListingWorked: vi.fn((_hostId: string) => false),
}))

vi.mock('#lib/tauri-commands/index.js', () => ({
  listSharesWithCredentials: h.listSharesWithCredentials,
  getSmbCredentials: h.getSmbCredentials,
  saveSmbCredentials: h.saveSmbCredentials,
  setSmbAccountPreference: h.setSmbAccountPreference,
  updateKnownShare: h.updateKnownShare,
  isUsingCredentialFileFallback: h.isUsingCredentialFileFallback,
  getUsernameHint: vi.fn(() => Promise.resolve(null)),
  getKnownShareByName: vi.fn(() => Promise.resolve(null)),
}))

vi.mock('./network-store.svelte', () => ({
  clearShareState: h.clearShareState,
  setShareState: h.setShareState,
  setCredentialStatus: h.setCredentialStatus,
  setListedAccount: h.setListedAccount,
  forgetCredentials: h.forgetCredentials,
  getListedAccount: h.getListedAccount,
  guestListingWorked: h.guestListingWorked,
}))

vi.mock('#lib/ui/toast/index.js', () => ({ addToast: h.addToast }))
vi.mock('#lib/servers/sign-in-sheet-state.svelte.js', () => ({ openSignInSheet: h.openSignInSheet }))
vi.mock('#lib/settings/network-settings.js', () => ({
  getNetworkTimeoutMs: () => 5000,
  getShareCacheTtlMs: () => 30000,
}))

const host: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'Testnas.local',
  ipAddress: '192.168.50.10',
  port: 445,
  source: 'discovered',
}
const server: ShareListServer = { host, label: 'My NAS' }
const testshare: ShareInfo = { name: 'testshare', isDisk: true, comment: null }
const listed = (authMode: ShareListResult['authMode']): ShareListResult => ({
  shares: [testshare],
  authMode,
  fromCache: false,
})

/** The sheet request under test, as much of it as these assert. */
interface SheetRequest {
  mode: string
  shape: { kind: string; guestAllowed?: boolean }
  endpoint: { address: string; host: string; displayName: string; username?: string }
  refusal?: string
  attempt: (submission: {
    mode: 'sign-in'
    secret: { secret: string; remember: boolean } | null
    username: string | null
  }) => Promise<{ kind: string; refusal?: string }>
}

/** The one request the round made of the sign-in sheet. */
function sheetRequest(): SheetRequest {
  expect(h.openSignInSheet, 'the sheet to have been asked for').toHaveBeenCalledTimes(1)
  return h.openSignInSheet.mock.calls[0][0] as SheetRequest
}

/** A sheet that runs `submission` once, then closes the way the sheet does on `answer`. */
function sheetAnswering(submission: Parameters<SheetRequest['attempt']>[0]) {
  h.openSignInSheet.mockImplementation(async (request: SheetRequest) => {
    const answer = await request.attempt(submission)
    return answer.kind === 'refused' ? { kind: 'cancelled' } : { kind: 'handed_off' }
  })
}

beforeEach(() => {
  vi.clearAllMocks()
  h.getListedAccount.mockReturnValue(undefined)
  h.guestListingWorked.mockReturnValue(false)
  h.getSmbCredentials.mockRejectedValue(new Error('not found'))
  h.openSignInSheet.mockResolvedValue({ kind: 'cancelled' })
})

describe('askToList: the LISTING needs a sign-in', () => {
  it('asks the sheet for a username and a password about the SERVER, naming it by its label', async () => {
    await askToList(server, { refusal: 'needs_credentials', guestAllowed: false })

    const request = sheetRequest()
    expect(request.mode).toBe('sign-in')
    expect(request.shape).toEqual({ kind: 'username_password', guestAllowed: false })
    // Listing auth is server-level: no share in the address.
    expect(request.endpoint.address).toBe('smb://Testnas')
    expect(request.endpoint.displayName).toBe('My NAS')
    expect(request.endpoint.host, 'the key the hints and the store use stays the host').toBe('Testnas')
    expect(request.refusal).toBe('needs_credentials')
  })

  it('offers guest where the listing allows one', async () => {
    await askToList(server, { refusal: 'needs_credentials', guestAllowed: true })
    expect(sheetRequest().shape).toEqual({ kind: 'username_password', guestAllowed: true })
  })

  it('lists with what the person typed, and remembers it only once the listing works', async () => {
    h.listSharesWithCredentials.mockResolvedValue(listed('creds_required'))
    const pending = askToList(server, { refusal: 'needs_credentials', guestAllowed: false })
    await vi.waitFor(() => {
      expect(h.openSignInSheet).toHaveBeenCalled()
    })

    const answer = await sheetRequest().attempt({
      mode: 'sign-in',
      secret: { secret: 'hunter2', remember: true },
      username: 'test_user_3',
    })

    expect(answer).toEqual({ kind: 'handed_off' })
    expect(h.listSharesWithCredentials).toHaveBeenCalledWith(
      'testnas-id',
      'Testnas.local',
      '192.168.50.10',
      445,
      'test_user_3',
      'hunter2',
      'typed',
      5000,
      30000,
    )
    expect(h.saveSmbCredentials).toHaveBeenCalledWith('Testnas', null, 'test_user_3', 'hunter2')
    await pending
  })

  it('keeps the sheet open on a password the server refused, and saves nothing', async () => {
    h.listSharesWithCredentials.mockRejectedValue(
      new ShareListFailure({ type: 'auth_failed', message: 'Invalid username or password' }),
    )
    void askToList(server, { refusal: 'needs_credentials', guestAllowed: false })
    await vi.waitFor(() => {
      expect(h.openSignInSheet).toHaveBeenCalled()
    })

    const answer = await sheetRequest().attempt({
      mode: 'sign-in',
      secret: { secret: 'wrong', remember: true },
      username: 'test_user_3',
    })

    expect(answer).toEqual({ kind: 'refused', refusal: 'authentication_rejected' })
    expect(h.saveSmbCredentials).not.toHaveBeenCalled()
    expect(h.setCredentialStatus).toHaveBeenCalledWith('Testnas', 'failed')
  })

  it('sends no account for guest, so the server is asked for exactly what was offered', async () => {
    h.listSharesWithCredentials.mockResolvedValue(listed('guest_allowed'))
    sheetAnswering({ mode: 'sign-in', secret: null, username: null })

    const outcome = await askToList(server, { refusal: 'needs_credentials', guestAllowed: true })

    expect(h.listSharesWithCredentials).toHaveBeenCalledWith(
      'testnas-id',
      'Testnas.local',
      '192.168.50.10',
      445,
      null,
      null,
      'typed',
      5000,
      30000,
    )
    expect(outcome).toEqual({ kind: 'listed', result: listed('guest_allowed'), credentials: null })
  })

  it('answers the listing that closed the sheet, with the account that listed', async () => {
    h.listSharesWithCredentials.mockResolvedValue(listed('creds_required'))
    sheetAnswering({ mode: 'sign-in', secret: { secret: 'hunter2', remember: false }, username: 'test_user_3' })

    const outcome = await askToList(server, { refusal: 'needs_credentials', guestAllowed: false })

    expect(outcome).toEqual({
      kind: 'listed',
      result: listed('creds_required'),
      credentials: { username: 'test_user_3', password: 'hunter2' },
    })
  })

  it('closes onto the share list’s own error state for a refusal about the SERVER', async () => {
    h.listSharesWithCredentials.mockRejectedValue(new ShareListFailure({ type: 'host_unreachable', message: '' }))
    sheetAnswering({ mode: 'sign-in', secret: { secret: 'hunter2', remember: false }, username: 'test_user_3' })

    const outcome = await askToList(server, { refusal: 'needs_credentials', guestAllowed: false })

    expect(outcome).toEqual({ kind: 'failed', error: { type: 'host_unreachable', message: '' } })
  })

  it('answers cancelled when the person closes the sheet', async () => {
    expect(await askToList(server, { refusal: 'needs_credentials', guestAllowed: false })).toEqual({
      kind: 'cancelled',
    })
  })
})

describe('listWithAccount: what a listing that worked leaves behind', () => {
  it('records the account everywhere the server is read from', async () => {
    h.listSharesWithCredentials.mockResolvedValue(listed('guest_allowed'))

    await listWithAccount(server, { username: 'testuser', password: 'testpass', remember: false }, 'typed')

    expect(h.clearShareState).toHaveBeenCalledWith('testnas-id')
    expect(h.setShareState).toHaveBeenCalledWith('testnas-id', listed('guest_allowed'))
    expect(h.setCredentialStatus).toHaveBeenCalledWith('Testnas', 'has_creds')
    expect(h.setListedAccount).toHaveBeenCalledWith('testnas-id', { kind: 'user', username: 'testuser' })
    // ❗ The choice sticks: it becomes the server's account, so later listings sign in as it.
    expect(h.setSmbAccountPreference).toHaveBeenCalledWith('Testnas', 'testuser')
    expect(h.updateKnownShare).toHaveBeenCalledWith('Testnas', '', 'credentials', 'guest_or_credentials', 'testuser')
    expect(h.saveSmbCredentials, 'not asked to remember').not.toHaveBeenCalled()
  })

  it('says so when a remembered password landed in the fallback file', async () => {
    h.listSharesWithCredentials.mockResolvedValue(listed('creds_required'))
    h.isUsingCredentialFileFallback.mockResolvedValueOnce(true)

    await listWithAccount(server, { username: 'testuser', password: 'testpass', remember: true }, 'typed')

    expect(h.addToast).toHaveBeenCalledWith(expect.any(String), { level: 'info', id: 'credential-file-fallback' })
  })
})

describe('the stored password', () => {
  it('lists with it as a SAVED credential, reading the Keychain once', async () => {
    h.getSmbCredentials.mockResolvedValue({ username: 'test_user_3', password: 'hunter2' })
    h.listSharesWithCredentials.mockResolvedValue(listed('creds_required'))

    const stored = await tryStoredCredentials(server)

    expect(h.getSmbCredentials).toHaveBeenCalledOnce()
    expect(h.getSmbCredentials).toHaveBeenCalledWith('Testnas', null)
    expect(stored?.credentials).toEqual({ username: 'test_user_3', password: 'hunter2' })
    expect(stored?.attempt.kind).toBe('listed')
    expect(h.listSharesWithCredentials).toHaveBeenCalledWith(
      'testnas-id',
      'Testnas.local',
      '192.168.50.10',
      445,
      'test_user_3',
      'hunter2',
      'saved',
      5000,
      30000,
    )
  })

  it('answers null, listing nothing, when none is saved', async () => {
    expect(await tryStoredCredentials(server)).toBeNull()
    expect(await readStoredCredentials(server)).toBeNull()
    expect(h.listSharesWithCredentials).not.toHaveBeenCalled()
  })
})

describe('the account actions', () => {
  it('Sign in as… opens with no refusal, no guest, prefilled with the listed account', async () => {
    await signInAs(server, { kind: 'user', username: 'testuser' })

    const request = sheetRequest()
    expect(request.shape).toEqual({ kind: 'username_password', guestAllowed: false })
    expect(request.refusal, 'nothing was refused: the person asked').toBeUndefined()
    expect(request.endpoint.username).toBe('testuser')
  })

  it('Browse as guest clears the account FIRST, then lists as guest', async () => {
    h.listSharesWithCredentials.mockResolvedValue(listed('guest_allowed'))

    const outcome = await useGuest(server)

    expect(h.setSmbAccountPreference).toHaveBeenCalledWith('Testnas', null)
    // ❗ While the account is set, the backend never lists this server as guest.
    expect(h.setSmbAccountPreference.mock.invocationCallOrder[0]).toBeLessThan(
      h.listSharesWithCredentials.mock.invocationCallOrder[0],
    )
    expect(h.listSharesWithCredentials).toHaveBeenCalledWith(
      'testnas-id',
      expect.anything(),
      expect.anything(),
      445,
      null,
      null,
      'typed',
      expect.anything(),
      expect.anything(),
    )
    expect(outcome.kind).toBe('listed')
  })

  it('Forget saved password forgets it and says so by the server’s label', async () => {
    expect(await forgetPassword(server)).toBe(true)
    expect(h.forgetCredentials).toHaveBeenCalledWith('Testnas')
    expect(h.addToast).toHaveBeenCalledWith(expect.stringContaining('My NAS'), { level: 'success' })
  })

  it('Forget saved password answers false, with a toast, when it did not go through', async () => {
    h.forgetCredentials.mockRejectedValueOnce(new Error('keychain locked'))
    expect(await forgetPassword(server)).toBe(false)
    expect(h.addToast).toHaveBeenCalledWith(expect.any(String), { level: 'error' })
  })
})

describe('one ask at a time', () => {
  it('asks nothing while a sheet is up, answering null, and asks again once it closes', async () => {
    let close: (answer: { kind: string }) => void = () => {}
    h.openSignInSheet.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          close = resolve
        }),
    )
    const first = askToList(server, { refusal: 'needs_credentials', guestAllowed: false })
    await vi.waitFor(() => {
      expect(h.openSignInSheet).toHaveBeenCalledOnce()
    })
    expect(isShareListSheetUp()).toBe(true)

    // ❗ A second sheet would close the first as `cancelled`, read as "the person said no".
    expect(await signInAs(server, undefined)).toBeNull()
    expect(await askToList(server, { refusal: 'needs_credentials', guestAllowed: false })).toBeNull()
    expect(h.openSignInSheet).toHaveBeenCalledOnce()

    close({ kind: 'cancelled' })
    expect(await first).toEqual({ kind: 'cancelled' })
    expect(isShareListSheetUp()).toBe(false)
    await signInAs(server, undefined)
    expect(h.openSignInSheet).toHaveBeenCalledTimes(2)
  })
})

describe('Forget saved password, when there was none', () => {
  /** Offered unconditionally (no Keychain read to decide), so the store's typed `not_found` answers. */
  it('says there was none, by the server’s label, and counts it as none stored', async () => {
    h.forgetCredentials.mockRejectedValueOnce(Object.assign(new Error('Credentials not found'), { type: 'not_found' }))
    expect(await forgetPassword(server)).toBe(true)
    expect(h.addToast).toHaveBeenCalledExactlyOnceWith('There was no saved password for My NAS.', { level: 'info' })
    expect(h.setCredentialStatus).toHaveBeenCalledWith('Testnas', 'no_creds')
  })
})

describe('runServerAccountAction: every door’s one runner', () => {
  it('Sign in as… prefills the account the server’s listings signed in as, and says it listed', async () => {
    h.getListedAccount.mockReturnValue({ kind: 'user', username: 'testuser' })
    h.listSharesWithCredentials.mockResolvedValue(listed('creds_required'))
    sheetAnswering({ mode: 'sign-in', secret: { secret: 'hunter2', remember: false }, username: 'testuser' })

    expect(await runServerAccountAction('sign-in-as', server)).toEqual({ listed: true })
    expect(sheetRequest().endpoint.username).toBe('testuser')
    expect(h.getListedAccount).toHaveBeenCalledWith('testnas-id')
  })

  it('says nothing and lists nothing when the person cancels', async () => {
    expect(await runServerAccountAction('sign-in-as', server)).toEqual({ listed: false })
    expect(h.addToast).not.toHaveBeenCalled()
  })

  it('words a refusal about the SERVER in the share list’s own words', async () => {
    h.listSharesWithCredentials.mockRejectedValue(new ShareListFailure({ type: 'host_unreachable', message: '' }))
    sheetAnswering({ mode: 'sign-in', secret: { secret: 'hunter2', remember: false }, username: 'testuser' })

    expect(await runServerAccountAction('sign-in-as', server)).toEqual({ listed: false })
    expect(h.addToast).toHaveBeenCalledExactlyOnceWith(expect.stringContaining('My NAS'), { level: 'error' })
  })

  it('Browse as guest lists as guest where a guest listing worked', async () => {
    h.guestListingWorked.mockReturnValue(true)
    h.listSharesWithCredentials.mockResolvedValue(listed('guest_allowed'))

    expect(await runServerAccountAction('browse-as-guest', server)).toEqual({ listed: true })
    expect(h.setSmbAccountPreference).toHaveBeenCalledWith('Testnas', null)
  })

  /** ❗ It drops the account FIRST, so where no guest got in it never starts, and says why. */
  it('Browse as guest keeps the account, and says why, where no guest listing worked', async () => {
    expect(await runServerAccountAction('browse-as-guest', server)).toEqual({ listed: false })
    expect(h.setSmbAccountPreference).not.toHaveBeenCalled()
    expect(h.listSharesWithCredentials).not.toHaveBeenCalled()
    expect(h.addToast).toHaveBeenCalledExactlyOnceWith(expect.stringContaining('My NAS'), { level: 'info' })
  })

  it('Forget saved password forgets, with no listing to open', async () => {
    expect(await runServerAccountAction('forget-secret', server)).toEqual({ listed: false })
    expect(h.forgetCredentials).toHaveBeenCalledWith('Testnas')
  })

  /** ❗ No door reads the Keychain to decide anything: the facts are this session's listings. */
  it('reads no password for any action', async () => {
    h.guestListingWorked.mockReturnValue(true)
    h.listSharesWithCredentials.mockResolvedValue(listed('guest_allowed'))
    for (const action of ['sign-in-as', 'browse-as-guest', 'forget-secret'] as const) {
      await runServerAccountAction(action, server)
    }
    expect(h.getSmbCredentials).not.toHaveBeenCalled()
  })
})
