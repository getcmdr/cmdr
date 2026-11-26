/**
 * Tests for `share-mount.svelte.ts`, mounting a share from a server's share list.
 *
 * Regression (the "Testnas dead end"): a mount that failed with an auth-class error
 * rendered a dead-end error pane whose "Try again" replayed the identical credentials.
 * Auth-class mount errors must ASK; answering retries the mount with the entered
 * credentials and saves them on success when "Remember" is checked.
 *
 * ❗ The asking is the one sign-in sheet, so these assert the REQUEST the controller makes
 * (its shape, its endpoint, and what its `attempt` calls), ❌ never a form in the pane.
 */

import { describe, it, expect, vi, beforeEach, afterEach, onTestFinished } from 'vitest'
import { flushSync } from 'svelte'
import { withPlatform } from '#lib/platform/test-platform.js'
import type { MountError, NetworkHost, ShareInfo } from '../types'
import { MountFailure } from '../network/mount-error'
import { renderMountError } from '../network/mount-error-messages'
import { createShareMount, MOUNT_SLOW_AFTER_MS, type ShareMount, type ShareMountServer } from './share-mount.svelte'

const h = vi.hoisted(() => ({
  mountNetworkShare: vi.fn(),
  cancelServerConnect: vi.fn(() => Promise.resolve(true)),
  saveSmbCredentials: vi.fn(() => Promise.resolve()),
  setSmbAccountPreference: vi.fn(() => Promise.resolve(true)),
  resolvePathVolume: vi.fn(),
  updateLeftPaneState: vi.fn((_state: unknown) => Promise.resolve()),
  openSignInSheet: vi.fn(),
  addToast: vi.fn(() => 'id'),
  warn: vi.fn(),
}))

vi.mock('#lib/tauri-commands/index.js', () => ({
  mountNetworkShare: h.mountNetworkShare,
  cancelServerConnect: h.cancelServerConnect,
  newServerAttemptId: () => 'server-connect-test-attempt',
  resolvePathVolume: h.resolvePathVolume,
  saveSmbCredentials: h.saveSmbCredentials,
  setSmbAccountPreference: h.setSmbAccountPreference,
  getUsernameHint: vi.fn(() => Promise.resolve(null)),
  getKnownShareByName: vi.fn(() => Promise.resolve(null)),
  updateLeftPaneState: h.updateLeftPaneState,
  updateRightPaneState: vi.fn(() => Promise.resolve()),
}))
vi.mock('#lib/servers/sign-in-sheet-state.svelte.js', () => ({ openSignInSheet: h.openSignInSheet }))
vi.mock('#lib/settings/network-settings.js', () => ({ getMountTimeoutMs: () => 15000 }))
vi.mock('#lib/logging/logger.js', () => ({
  getAppLogger: () => ({ debug: vi.fn(), info: vi.fn(), warn: h.warn, error: vi.fn() }),
}))
vi.mock('#lib/ui/toast/index.js', () => ({ addToast: h.addToast }))

/** The address the controller mounts by, which is NOT the name a person knows the host by. */
const ADDRESS = '192.168.50.10'

const host: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'Testnas.local',
  ipAddress: ADDRESS,
  port: 445,
  source: 'discovered',
}

const testshare: ShareInfo = { name: 'testshare', isDisk: true, comment: null }

const authFailed: MountError = { type: 'auth_failed', server: ADDRESS }
const authRequired: MountError = { type: 'auth_required', server: ADDRESS, share: 'testshare' }
const accountRefused: MountError = {
  type: 'permission_denied',
  server: ADDRESS,
  share: 'testshare',
  username: 'test_user_3',
}
const unreachable: MountError = { type: 'host_unreachable', server: ADDRESS }

const refused = (error: MountError) => new MountFailure(error)

/** The sheet request under test, as much of it as these assert. */
interface SheetRequest {
  shape: { kind: string; guestAllowed?: boolean }
  endpoint: { address: string }
  refusal?: string
  attempt: (submission: {
    mode: 'sign-in'
    secret: { secret: string; remember: boolean } | null
    username: string | null
  }) => Promise<{ kind: string; refusal?: string }>
}

function sheetRequest(): SheetRequest {
  expect(h.openSignInSheet, 'the sheet to have been asked for').toHaveBeenCalledTimes(1)
  return h.openSignInSheet.mock.calls[0][0] as SheetRequest
}

let dispose: (() => void) | undefined

afterEach(() => {
  dispose?.()
  dispose = undefined
  vi.useRealTimers()
})

/** A controller on `server`, with every dep a spy. */
function create(server: ShareMountServer | null = { host, label: undefined }) {
  let current = $state<ShareMountServer | null>(server)
  const deps = {
    onMounted: vi.fn(() => {
      current = null
    }),
    onVolumeChange: vi.fn(),
    onPendingConsumed: vi.fn(),
  }
  let mount!: ShareMount
  dispose = $effect.root(() => {
    mount = createShareMount({ getPaneId: () => 'left', getServer: () => current, ...deps })
  })
  flushSync()
  return { mount, ...deps }
}

beforeEach(() => {
  vi.clearAllMocks()
  h.resolvePathVolume.mockResolvedValue({ volume: null })
  h.openSignInSheet.mockResolvedValue({ kind: 'cancelled' })
})

describe('a mount a credential answers', () => {
  it('asks the sheet for this share, with no guest option, in the app’s vocabulary', async () => {
    h.mountNetworkShare.mockRejectedValue(refused(authFailed))
    const { mount } = create()

    await mount.mount(testshare, null)

    const request = sheetRequest()
    // ❗ No guest option: guest is what the mount just tried.
    expect(request.shape).toEqual({ kind: 'username_password', guestAllowed: false })
    expect(request.endpoint.address).toBe('smb://Testnas/testshare')
    expect(request.refusal).toBe('authentication_rejected')
  })

  it('retries the mount with entered credentials and saves them on success', async () => {
    h.mountNetworkShare
      .mockRejectedValueOnce(refused(authFailed))
      .mockResolvedValueOnce({ mountPath: '/Volumes/testshare', alreadyMounted: false })
    h.openSignInSheet.mockReturnValue(new Promise(() => {}))
    const { mount } = create()
    void mount.mount(testshare, null)
    await vi.waitFor(() => {
      expect(h.openSignInSheet).toHaveBeenCalled()
    })

    const outcome = await sheetRequest().attempt({
      mode: 'sign-in',
      secret: { secret: 'hunter2', remember: true },
      username: 'test_user_3',
    })

    expect(outcome).toEqual({ kind: 'handed_off' })
    // The host's own name rides along, which is what the saved share is filed under.
    expect(h.mountNetworkShare).toHaveBeenLastCalledWith(
      ADDRESS,
      'testshare',
      'test_user_3',
      'hunter2',
      445,
      15000,
      'Testnas',
      'server-connect-test-attempt',
    )
    expect(h.saveSmbCredentials).toHaveBeenCalledWith('Testnas', null, 'test_user_3', 'hunter2')
    // cmdr-reports#27: the account that got in becomes the host's account.
    expect(h.setSmbAccountPreference).toHaveBeenCalledWith('Testnas', 'test_user_3')
  })

  it('keeps the sheet open on a second refusal, and saves nothing', async () => {
    h.mountNetworkShare.mockRejectedValue(refused(authFailed))
    h.openSignInSheet.mockReturnValue(new Promise(() => {}))
    const { mount } = create()
    void mount.mount(testshare, null)
    await vi.waitFor(() => {
      expect(h.openSignInSheet).toHaveBeenCalled()
    })

    const outcome = await sheetRequest().attempt({
      mode: 'sign-in',
      secret: { secret: 'wrong', remember: true },
      username: 'test_user_3',
    })

    expect(outcome).toEqual({ kind: 'refused', refusal: 'authentication_rejected' })
    expect(h.saveSmbCredentials).not.toHaveBeenCalled()
    expect(h.setSmbAccountPreference, 'a refused account is not the host account').not.toHaveBeenCalled()
  })

  it('says a password is needed, calmly, when the share refuses guests', async () => {
    // ERR-SHUSC: a share guests can SEE but can't OPEN. ❗ Nothing was offered, so it
    // isn't a rejected password, and the state behind the sheet is the calm sign-in one:
    // no Try again, which without an account gets the same answer (cmdr-reports#10).
    h.mountNetworkShare.mockRejectedValue(refused(authRequired))
    h.openSignInSheet.mockReturnValue(new Promise(() => {}))
    const { mount } = create({ host, label: 'My NAS' })
    void mount.mount(testshare, null)
    await vi.waitFor(() => {
      expect(h.openSignInSheet).toHaveBeenCalled()
    })

    expect(sheetRequest().refusal).toBe('needs_credentials')
    const state = mount.view?.state
    expect(state?.kind).toBe('signed_out')
    if (state?.kind !== 'signed_out') return
    expect(state.title).toBe('Sign in to open “testshare”')
    expect(state.hint).toContain('“My NAS”')
    expect(state.signIn).toBeTypeOf('function')
    expect(state.back).toBeTypeOf('function')
  })

  it('asks for a different account when the share refuses the one that signed in', async () => {
    h.mountNetworkShare.mockRejectedValueOnce(refused(authRequired)).mockRejectedValueOnce(refused(accountRefused))
    h.openSignInSheet.mockReturnValue(new Promise(() => {}))
    const { mount } = create()
    void mount.mount(testshare, null)
    await vi.waitFor(() => {
      expect(h.openSignInSheet).toHaveBeenCalled()
    })

    const outcome = await sheetRequest().attempt({
      mode: 'sign-in',
      secret: { secret: 'hunter2', remember: true },
      username: 'test_user_3',
    })

    // ❗ Not `authentication_rejected`: the password worked.
    expect(outcome).toEqual({ kind: 'refused', refusal: 'account_not_permitted' })
    expect(h.saveSmbCredentials).not.toHaveBeenCalled()
  })

  it('hands a refusal about the SHARE back to the pane, which has the words and the retry', async () => {
    h.mountNetworkShare
      .mockRejectedValueOnce(refused(authFailed))
      .mockRejectedValueOnce(refused({ type: 'share_not_found', server: ADDRESS, share: 'testshare' }))
    h.openSignInSheet.mockReturnValue(new Promise(() => {}))
    const { mount } = create()
    void mount.mount(testshare, null)
    await vi.waitFor(() => {
      expect(h.openSignInSheet).toHaveBeenCalled()
    })

    const outcome = await sheetRequest().attempt({
      mode: 'sign-in',
      secret: { secret: 'hunter2', remember: false },
      username: 'test_user_3',
    })

    expect(outcome).toEqual({ kind: 'handed_off' })
    expect(mount.view?.state.kind).toBe('refused')
  })

  it('goes back to the share list when the sign-in is cancelled', async () => {
    h.mountNetworkShare.mockRejectedValue(refused(authFailed))
    const { mount } = create()

    await mount.mount(testshare, null)

    expect(mount.view, 'the share list is where cancelling lands').toBeNull()
  })
})

describe('a mount that stopped short', () => {
  it('words the refusal from the catalog, naming the host by its own name, with Try again and Go back', async () => {
    const notFound: MountError = { type: 'share_not_found', server: ADDRESS, share: 'testshare' }
    h.mountNetworkShare.mockRejectedValue(refused(notFound))
    const { mount } = create()

    await mount.mount(testshare, null)

    const state = mount.view?.state
    expect(state?.kind).toBe('refused')
    if (state?.kind !== 'refused') return
    expect(state.refusal).toBe(renderMountError(notFound, 'Testnas'))
    expect(state.refusal).not.toContain(ADDRESS)
    expect(state.retry).toBeTypeOf('function')
    expect(state.back).toBeTypeOf('function')
    expect(h.openSignInSheet, 'no credential answers it').not.toHaveBeenCalled()
  })

  it('logs a refusal as readable text, not "[object Object]"', async () => {
    h.mountNetworkShare.mockRejectedValue(refused({ type: 'share_not_found', server: ADDRESS, share: 'testshare' }))
    const { mount } = create()

    await mount.mount(testshare, null)

    const [template, properties] = h.warn.mock.calls[0] as [string, Record<string, unknown>]
    expect(template).toContain('{error}')
    expect(String(properties.error)).toContain('share_not_found')
  })

  it('words a mount call that broke down without a typed refusal as the catch-all', async () => {
    h.mountNetworkShare.mockRejectedValue(new Error('IPC transport closed'))
    const { mount } = create()

    await mount.mount(testshare, null)

    const state = mount.view?.state
    if (state?.kind !== 'refused') throw new Error('expected a refusal')
    expect(state.refusal).toBe(
      renderMountError({ type: 'unexpected', server: ADDRESS, share: 'testshare', detail: 'ignored' }, 'Testnas'),
    )
    expect(state.refusal).not.toContain('IPC transport')
    expect(h.openSignInSheet).not.toHaveBeenCalled()
  })

  it('Try again mounts again, and a mount that goes through leaves the refusal', async () => {
    h.mountNetworkShare
      .mockRejectedValueOnce(refused({ type: 'mount_missing', server: ADDRESS, share: 'testshare' }))
      .mockResolvedValueOnce({ mountPath: '/Volumes/testshare', alreadyMounted: false })
    const { mount, onMounted, onVolumeChange } = create()
    await mount.mount(testshare, null)

    const state = mount.view?.state
    if (state?.kind !== 'refused') throw new Error('expected a refusal')
    state.retry?.()

    await vi.waitFor(() => {
      expect(onVolumeChange).toHaveBeenCalledWith({
        volumeId: '/Volumes/testshare',
        volumePath: '/Volumes/testshare',
        targetPath: '/Volumes/testshare',
      })
    })
    expect(h.mountNetworkShare).toHaveBeenCalledTimes(2)
    expect(onMounted).toHaveBeenCalledOnce()
    expect(mount.view).toBeNull()
  })

  it('mirrors the refusal into the pane state MCP reads, and clears it on the way back', async () => {
    h.mountNetworkShare.mockRejectedValue(refused(unreachable))
    const { mount } = create()
    await mount.mount(testshare, null)
    flushSync()

    expect(h.updateLeftPaneState).toHaveBeenCalledWith(
      expect.objectContaining({
        volumeId: 'network',
        mountError: {
          share: 'testshare',
          reason: 'host_unreachable',
          message: renderMountError(unreachable, 'Testnas'),
        },
      }),
    )

    mount.back()
    flushSync()
    // An explicit `null`: the share list's own push omits the key, so it can't clear it.
    expect(h.updateLeftPaneState).toHaveBeenLastCalledWith(expect.objectContaining({ mountError: null }))
    expect(mount.view).toBeNull()
  })

  it('answers the keyboard there, one step back to the share list', async () => {
    h.mountNetworkShare.mockRejectedValue(refused(unreachable))
    const { mount } = create()
    await mount.mount(testshare, null)

    const event = new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true })
    expect(mount.handleKeyDown(event), 'the refusal owns the keyboard').toBe(true)
    expect(event.defaultPrevented).toBe(true)
    expect(mount.view).toBeNull()
    // Back on the share list, keys go to it again.
    expect(mount.handleKeyDown(new KeyboardEvent('keydown', { key: 'ArrowDown' }))).toBe(false)
  })

  it('drops the refusal and its retry for another host', async () => {
    h.mountNetworkShare.mockRejectedValue(refused(unreachable))
    const { mount } = create()
    await mount.mount(testshare, null)

    mount.reset()
    expect(mount.view).toBeNull()
  })
})

/**
 * #604: a NetFS mount can hang behind a stuck macOS sign-in helper for as long as the
 * timeout allows. The wait says what's going on and can be called off.
 */
describe('a mount that waits', () => {
  let answerMount: (error: MountError) => void = () => {}

  beforeEach(() => {
    h.mountNetworkShare.mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          answerMount = (error) => {
            reject(refused(error))
          }
        }),
    )
  })

  it('names the share, the server by its friendly name, and the address it dials', () => {
    const { mount } = create({ host, label: 'My NAS' })
    void mount.mount(testshare, null)

    expect(mount.view).toMatchObject({ name: 'testshare', server: 'My NAS', address: `${ADDRESS}:445` })
    expect(mount.view?.state.kind).toBe('connecting')
  })

  it('names the share alone for a host known only by its address, so the address shows once', () => {
    const ipOnly: NetworkHost = { id: 'manual-ip', name: ADDRESS, ipAddress: ADDRESS, port: 445, source: 'manual' }
    const { mount } = create({ host: ipOnly, label: undefined })
    void mount.mount(testshare, null)

    expect(mount.view).toMatchObject({ name: 'testshare', server: null, address: `${ADDRESS}:445` })
  })

  it('Cancel calls off that mount and goes back to the share list quietly', async () => {
    const { mount } = create()
    const pending = mount.mount(testshare, null)
    const state = mount.view?.state
    if (state?.kind !== 'connecting') throw new Error('expected the wait')

    state.cancel()
    expect(h.cancelServerConnect).toHaveBeenCalledWith('server-connect-test-attempt')
    answerMount({ type: 'cancelled', share: 'testshare' })
    await pending

    expect(mount.view, 'a cancel the person asked for is no refusal').toBeNull()
  })

  it('cancels on the back key too, and every other key is dead while it waits', () => {
    const { mount } = create()
    void mount.mount(testshare, null)

    expect(mount.handleKeyDown(new KeyboardEvent('keydown', { key: 'ArrowDown' }))).toBe(true)
    expect(h.cancelServerConnect).not.toHaveBeenCalled()
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    expect(mount.handleKeyDown(escape)).toBe(true)
    expect(escape.defaultPrevented).toBe(true)
    expect(h.cancelServerConnect).toHaveBeenCalledWith('server-connect-test-attempt')
  })

  it.each([
    ['macos', 'Still waiting for macOS to connect. You can cancel at any time.'],
    ['windows', 'Still waiting for Windows to connect. You can cancel at any time.'],
    ['linux', 'Still connecting to the share. You can cancel at any time.'],
  ] as const)('says it is still waiting once the mount runs past ten seconds (%s)', (platform, slow) => {
    onTestFinished(withPlatform(platform))
    vi.useFakeTimers()
    const { mount } = create()
    void mount.mount(testshare, null)
    const hint = () => (mount.view?.state.kind === 'connecting' ? mount.view.state.hint : 'not waiting')

    expect(hint(), 'a fresh mount says only that it takes a moment').toBeUndefined()
    vi.advanceTimersByTime(MOUNT_SLOW_AFTER_MS)
    expect(hint()).toBe(slow)
  })

  it('explains a stuck sign-in helper, with the command that restarts it', async () => {
    const { mount } = create()
    const pending = mount.mount(testshare, null)
    answerMount({ type: 'sign_in_helper_stuck', server: ADDRESS })
    await pending

    const state = mount.view?.state
    if (state?.kind !== 'refused') throw new Error('expected a refusal')
    expect(state.refusal).toContain('Testnas')
    expect(state.refusal).toContain('killall NetAuthSysAgent')
  })
})

/**
 * ❗ cmdr-reports#7: "Cmdr tried to open the share on that server." A queued share is a
 * one-shot for the visit it was queued with.
 */
describe('the queued share', () => {
  it('adopts the pane’s value once, and the same value again only after the pane cleared it', () => {
    const { mount } = create()
    mount.adoptQueued('testshare')
    expect(mount.pendingMount).toBe('testshare')

    mount.retirePending()
    mount.adoptQueued('testshare')
    expect(mount.pendingMount, 'a retired target is not re-adopted').toBeUndefined()

    mount.adoptQueued(undefined)
    mount.adoptQueued('testshare')
    expect(mount.pendingMount).toBe('testshare')
  })

  it('is spent the moment the list hands it to the mount, and the pane hears so', () => {
    const { mount, onPendingConsumed } = create()
    mount.adoptQueued('TestShare')

    expect(mount.takePending([testshare])).toBe(testshare)
    expect(mount.pendingMount).toBeUndefined()
    expect(onPendingConsumed).toHaveBeenCalledOnce()
    // A list that comes back (after a Cancel, a refused mount) finds nothing queued.
    expect(mount.takePending([testshare])).toBeNull()
  })

  it('says the share wasn’t found on a list that lacks it, and is spent all the same', () => {
    const { mount } = create({ host, label: 'My NAS' })
    mount.queue('missing')

    expect(mount.takePending([testshare])).toBeNull()
    expect(h.addToast).toHaveBeenCalledWith(expect.stringContaining('missing'), { level: 'warn' })
    expect(mount.pendingMount).toBeUndefined()
  })

  it('is retired by a mount that went through', async () => {
    h.mountNetworkShare.mockResolvedValue({ mountPath: '/Volumes/other', alreadyMounted: false })
    const { mount, onPendingConsumed } = create()
    mount.queue('testshare')

    await mount.mount({ ...testshare, name: 'other' }, null)

    expect(mount.pendingMount).toBeUndefined()
    expect(onPendingConsumed).toHaveBeenCalled()
  })
})
