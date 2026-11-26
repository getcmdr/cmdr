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
  askToSignInToList,
  forgetPassword,
  isShareListSheetUp,
  listWithAccount,
  runServerAccountAction,
  signInAs,
  useGuest,
  type ShareListServer,
} from './share-list-sign-in'

const h = vi.hoisted(() => ({
  listSharesWithCredentials: vi.fn(),
  saveSmbCredentials: vi.fn(() => Promise.resolve()),
  setSmbAccountPreference: vi.fn(() => Promise.resolve(true)),
  updateKnownShare: vi.fn(() => Promise.resolve()),
  isUsingCredentialFileFallback: vi.fn(() => Promise.resolve(false)),
  openSignInSheet: vi.fn(),
  setCredentialStatus: vi.fn(),
  setListedAccount: vi.fn(),
  forgetCredentials: vi.fn(() => Promise.resolve()),
  addToast: vi.fn(() => 'id'),
  getListedAccount: vi.fn((_hostId: string): unknown => undefined),
  guestListingWorked: vi.fn((_hostId: string) => false),
  shareListHost: vi.fn((_path: string): Promise<unknown> => Promise.resolve(null)),
  systemSavedSmbAccount: vi.fn((_host: NetworkHost): Promise<{ username: string; canTry: boolean } | null> =>
    Promise.resolve(null),
  ),
  listSharesWithSystemSavedPassword: vi.fn(),
}))

vi.mock('#lib/tauri-commands/index.js', () => ({
  listSharesWithCredentials: h.listSharesWithCredentials,
  saveSmbCredentials: h.saveSmbCredentials,
  setSmbAccountPreference: h.setSmbAccountPreference,
  updateKnownShare: h.updateKnownShare,
  isUsingCredentialFileFallback: h.isUsingCredentialFileFallback,
  getUsernameHint: vi.fn(() => Promise.resolve(null)),
  getKnownShareByName: vi.fn(() => Promise.resolve(null)),
  shareListHost: h.shareListHost,
  systemSavedSmbAccount: h.systemSavedSmbAccount,
  listSharesWithSystemSavedPassword: h.listSharesWithSystemSavedPassword,
}))

vi.mock('./network-store.svelte', () => ({
  getResolvedShareListHost: () => undefined,
  noteResolvedShareListHost: vi.fn(),
  setCredentialStatus: h.setCredentialStatus,
  setListedAccount: h.setListedAccount,
  forgetCredentials: h.forgetCredentials,
  getListedAccount: h.getListedAccount,
  guestListingWorked: h.guestListingWorked,
  getNetworkHosts: () => [host],
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
  h.openSignInSheet.mockResolvedValue({ kind: 'cancelled' })
  h.systemSavedSmbAccount.mockResolvedValue(null)
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
    expect(outcome).toEqual({ kind: 'listed', result: listed('guest_allowed') })
  })

  it('answers the listing that closed the sheet, and ❗ hands no password back', async () => {
    h.listSharesWithCredentials.mockResolvedValue(listed('creds_required'))
    sheetAnswering({ mode: 'sign-in', secret: { secret: 'hunter2', remember: false }, username: 'test_user_3' })

    const outcome = await askToList(server, { refusal: 'needs_credentials', guestAllowed: false })

    // The backend keeps the typed account as the server's session credential.
    expect(outcome).toEqual({ kind: 'listed', result: listed('creds_required') })
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

    expect(h.setCredentialStatus).toHaveBeenCalledWith('Testnas', 'has_creds')
    expect(h.setListedAccount).toHaveBeenCalledWith('testnas-id', { kind: 'user', username: 'testuser' })
    // ❗ The choice sticks: it becomes the server's account, so later listings sign in as it.
    expect(h.setSmbAccountPreference).toHaveBeenCalledWith(host, 'testuser')
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

describe('askToSignInToList: the error screen’s Sign in, and the ask a refused listing raises', () => {
  it('asks the sheet about the server the share-list path names, with the refusal that asked and no guest', async () => {
    await askToSignInToList('smb://testnas', { type: 'auth_failed', message: '' })

    const request = sheetRequest()
    expect(request.refusal).toBe('authentication_rejected')
    expect(request.shape.guestAllowed).toBe(false)
  })

  it('asks about a server saved without contact (no host here) by the host the backend lists it under', async () => {
    // Discovery never lists a server saved without contact, so only the backend's resolver
    // knows its host, by the id the share cache and the session credential are keyed by.
    const saved: NetworkHost = {
      id: 'manual-TestNAS-local-445',
      name: 'TestNAS.local',
      hostname: 'TestNAS.local',
      port: 445,
      source: 'manual',
    }
    h.shareListHost.mockResolvedValueOnce(saved)

    await askToSignInToList('smb://testnas.local', { type: 'auth_required', message: '' })

    expect(h.shareListHost).toHaveBeenCalledWith('smb://testnas.local')
    expect(sheetRequest().endpoint.host).toBe('TestNAS.local')
  })

  it('asks nothing for a path that names no server this session knows', async () => {
    expect(await askToSignInToList('smb://gone-nas', { type: 'auth_required', message: '' })).toBeNull()
    expect(h.openSignInSheet).not.toHaveBeenCalled()
  })
})

describe('the password macOS saved', () => {
  const macosSaved = (canTry: boolean) => ({ username: 'test_user_1', canTry })

  it('is tried on its own when a share list someone opened needs a sign-in, and the sheet never opens', async () => {
    h.systemSavedSmbAccount.mockResolvedValue(macosSaved(true))
    h.listSharesWithSystemSavedPassword.mockResolvedValue(listed('creds_required'))

    const outcome = await askToSignInToList('smb://testnas', { type: 'auth_required', message: '' })

    expect(outcome).toEqual({ kind: 'listed', result: listed('creds_required') })
    expect(h.listSharesWithSystemSavedPassword).toHaveBeenCalledWith(host)
    expect(h.openSignInSheet).not.toHaveBeenCalled()
  })

  it('is not tried when Cmdr’s own password was what got turned down', async () => {
    h.systemSavedSmbAccount.mockResolvedValue(macosSaved(true))

    await askToSignInToList('smb://testnas', { type: 'auth_failed', message: '' })

    expect(h.listSharesWithSystemSavedPassword).not.toHaveBeenCalled()
    expect(h.openSignInSheet).toHaveBeenCalledTimes(1)
  })

  it('is not tried again once this run asked macOS for it: the sheet asks, prefilled, with no button', async () => {
    h.systemSavedSmbAccount.mockResolvedValue(macosSaved(false))

    await askToSignInToList('smb://testnas', { type: 'auth_required', message: '' })

    expect(h.listSharesWithSystemSavedPassword).not.toHaveBeenCalled()
    const request = h.openSignInSheet.mock.calls[0][0] as SheetRequest & { alternative?: unknown }
    expect(request.endpoint.username).toBe('test_user_1')
    expect(request.alternative, '❌ no inert affordance').toBeUndefined()
  })

  it('falls to the sheet when the server turns it down', async () => {
    h.systemSavedSmbAccount.mockResolvedValue(macosSaved(true))
    h.listSharesWithSystemSavedPassword.mockRejectedValue(
      new ShareListFailure({ type: 'auth_failed', message: 'rejected' }),
    )

    await askToSignInToList('smb://testnas', { type: 'auth_required', message: '' })

    expect(h.openSignInSheet).toHaveBeenCalledTimes(1)
  })

  it('is the sheet’s “Use the password macOS saved” where it may still be tried, and its listing closes the sheet', async () => {
    h.systemSavedSmbAccount.mockResolvedValue(macosSaved(true))
    h.listSharesWithSystemSavedPassword.mockResolvedValue(listed('creds_required'))
    h.openSignInSheet.mockImplementation(
      async (request: { alternative?: { label: string; attempt: () => Promise<{ kind: string }> } }) => {
        expect(request.alternative?.label).toBe('Use the password macOS saved')
        const answer = await request.alternative?.attempt()
        expect(answer).toEqual({ kind: 'handed_off' })
        return { kind: 'handed_off' }
      },
    )

    const outcome = await signInAs(server, undefined)

    expect(outcome).toEqual({ kind: 'listed', result: listed('creds_required') })
  })

  it('answers the button with the fields when macOS gave nothing after all', async () => {
    h.systemSavedSmbAccount.mockResolvedValue(macosSaved(true))
    h.listSharesWithSystemSavedPassword.mockResolvedValue(null)
    let answer: unknown
    h.openSignInSheet.mockImplementation(async (request: { alternative?: { attempt: () => Promise<unknown> } }) => {
      answer = await request.alternative?.attempt()
      return { kind: 'cancelled' }
    })

    await signInAs(server, undefined)

    expect(answer).toEqual({ kind: 'refused', refusal: 'needs_credentials' })
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

    expect(h.setSmbAccountPreference).toHaveBeenCalledWith(host, null)
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
    expect(h.setSmbAccountPreference).toHaveBeenCalledWith(host, null)
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
})
