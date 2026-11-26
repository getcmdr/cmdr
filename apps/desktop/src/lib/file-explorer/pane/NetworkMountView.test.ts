/**
 * Behavior tests for how NetworkMountView wires the share mount
 * (`share-mount.svelte.ts`, whose own tests pin the flows) and the share list.
 *
 * ❗ The asking is the one sign-in sheet, so these assert the REQUEST the view makes
 * (its endpoint, its refusal), ❌ never a form in the pane.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, unmount, tick } from 'svelte'
import NetworkMountView from './NetworkMountView.svelte'
import type { MountError, NetworkHost, ShareInfo } from '../types'
import { MountFailure } from '../network/mount-error'
import { renderMountError } from '../network/mount-error-messages'

const h = vi.hoisted(() => ({
  fetchShares: vi.fn(),
  mountNetworkShare: vi.fn(),
  cancelServerConnect: vi.fn(() => Promise.resolve(true)),
  saveSmbCredentials: vi.fn(),
  setSmbAccountPreference: vi.fn(() => Promise.resolve(true)),
  getSmbCredentials: vi.fn(),
  resolvePathVolume: vi.fn(),
  updateLeftPaneState: vi.fn(() => Promise.resolve()),
  openSignInSheet: vi.fn(),
  warn: vi.fn(),
  hosts: [] as NetworkHost[],
}))

vi.mock('#lib/tauri-commands/index.js', () => ({
  mountNetworkShare: h.mountNetworkShare,
  cancelServerConnect: h.cancelServerConnect,
  newServerAttemptId: () => 'server-connect-test-attempt',
  resolvePathVolume: h.resolvePathVolume,
  saveSmbCredentials: h.saveSmbCredentials,
  setSmbAccountPreference: h.setSmbAccountPreference,
  getSmbCredentials: h.getSmbCredentials,
  listSharesWithCredentials: vi.fn(() => Promise.resolve({ shares: [], authMode: 'unknown', fromCache: false })),
  isUsingCredentialFileFallback: vi.fn(() => Promise.resolve(false)),
  updateKnownShare: vi.fn(() => Promise.resolve()),
  getUsernameHint: vi.fn(() => Promise.resolve(null)),
  getKnownShareByName: vi.fn(() => Promise.resolve(null)),
  updateLeftPaneState: h.updateLeftPaneState,
  updateRightPaneState: vi.fn(() => Promise.resolve()),
  showNetworkHostContextMenu: vi.fn(() => Promise.resolve()),
  onNetworkHostContextAction: vi.fn(() => Promise.resolve(() => {})),
  disconnectNetworkHost: vi.fn(() => Promise.resolve()),
  connectToServer: vi.fn(() => Promise.resolve()),
  listSavedServers: vi.fn(() => Promise.resolve([])),
  notifyDialogOpened: vi.fn(() => Promise.resolve()),
  notifyDialogClosed: vi.fn(() => Promise.resolve()),
}))

vi.mock('#lib/servers/sign-in-sheet-state.svelte.js', () => ({ openSignInSheet: h.openSignInSheet }))

vi.mock('#lib/settings/network-settings.js', () => ({
  getMountTimeoutMs: () => 15000,
  getNetworkTimeoutMs: () => 5000,
  getShareCacheTtlMs: () => 30000,
}))

vi.mock('#lib/logging/logger.js', () => ({
  getAppLogger: () => ({ debug: vi.fn(), info: vi.fn(), warn: h.warn, error: vi.fn() }),
}))

vi.mock('../network/network-store.svelte', () => ({
  getNetworkHosts: () => h.hosts,
  getDiscoveryState: () => 'idle',
  holdDiscoveryForServersView: () => () => {},
  isHostResolving: () => false,
  getShareState: () => undefined,
  getShareCount: () => null,
  isListingShares: () => false,
  isShareDataStale: () => false,
  refreshAllStaleShares: vi.fn(),
  clearShareState: vi.fn(),
  setShareState: vi.fn(),
  setCredentialStatus: vi.fn(),
  noteCachedCredentials: vi.fn(() => Promise.resolve()),
  fetchShares: h.fetchShares,
  getCredentialStatus: () => 'unknown',
  getListedAccount: () => undefined,
  setListedAccount: vi.fn(),
  checkCredentialsForHost: vi.fn(() => Promise.resolve()),
  forgetCredentials: vi.fn(() => Promise.resolve()),
}))

vi.mock('#lib/utils/confirm-dialog.js', () => ({ confirmDialog: vi.fn(() => Promise.resolve(false)) }))
vi.mock('#lib/ui/toast/index.js', () => ({ addToast: vi.fn(() => 'id') }))
vi.mock('../network/lazy-trigger', () => ({ triggerNetworkDiscovery: vi.fn() }))

/** The address the view mounts by, which is NOT the name a person knows the host by. */
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

/** What `mountNetworkShare` throws for a refusal: the typed value, carried across the throw. */
function refused(error: MountError): MountFailure {
  return new MountFailure(error)
}

const authFailed: MountError = { type: 'auth_failed', server: ADDRESS }
const authRequired: MountError = { type: 'auth_required', server: ADDRESS, share: 'testshare' }
const unreachable: MountError = { type: 'host_unreachable', server: ADDRESS }

/** The sheet request under test, as much of it as these assert. */
interface SheetRequest {
  endpoint: { address: string }
  refusal?: string
}

/** The one request the view made of the sign-in sheet. */
function sheetRequest(): SheetRequest {
  expect(h.openSignInSheet, 'the sheet to have been asked for').toHaveBeenCalledTimes(1)
  return h.openSignInSheet.mock.calls[0][0] as SheetRequest
}

/** The exported NetworkMountView API surface the tests drive. */
interface NetworkMountViewApi {
  openCursorItem: () => void
  handleKeyDown: (e: KeyboardEvent) => void
  getItemCount: () => number
}

/** Narrows a queried element, failing the test with a readable message when absent. */
function must<T>(value: T | null | undefined, what: string): T {
  expect(value, `expected ${what} to be present`).toBeTruthy()
  return value as T
}

/** Mounts the view on the given host and activates the first share. */
async function mountViewAndActivateShare() {
  const target = document.createElement('div')
  document.body.appendChild(target)
  const component = mount(NetworkMountView, {
    target,
    props: { paneId: 'left', isFocused: true, initialNetworkHost: host },
  })
  const api = component as unknown as NetworkMountViewApi
  await vi.waitFor(() => {
    expect(target.querySelector('.share-row')).toBeTruthy()
  })
  // Down from `..`, where the share list opens, to the share.
  api.handleKeyDown(new KeyboardEvent('keydown', { key: 'ArrowDown' }))
  api.openCursorItem()
  await tick()
  return { target, component, api }
}

/** The buttons the pane's remote-connect view offers, by label. */
function buttonLabels(target: HTMLElement): string[] {
  return [...target.querySelectorAll('.remote-connect button')].map((b) => b.textContent.trim())
}

/** The remote-connect view's button labelled `label`. */
function button(target: HTMLElement, label: string): HTMLButtonElement {
  return must(
    [...target.querySelectorAll<HTMLButtonElement>('.remote-connect button')].find(
      (b) => b.textContent.trim() === label,
    ),
    `the ${label} button`,
  )
}

// The flows themselves (the sheet rounds, the retries, the words, the mirror, the queue)
// are `share-mount.svelte.test.ts`'s. These pin how the view wires them.
describe('NetworkMountView: a mount that stopped short', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    document.body.innerHTML = ''
    // Guest listing succeeded; no creds in play (the incident state).
    h.fetchShares.mockResolvedValue({ shares: [testshare], authMode: 'guest_allowed', fromCache: false })
    h.getSmbCredentials.mockRejectedValue(new Error('not found'))
    h.resolvePathVolume.mockResolvedValue({ volume: null })
    h.openSignInSheet.mockResolvedValue({ kind: 'cancelled' })
  })

  it('asks the sheet for this share (not a dead-end pane) on an auth-class mount error', async () => {
    h.mountNetworkShare.mockRejectedValue(refused(authFailed))
    const { target, component } = await mountViewAndActivateShare()

    await vi.waitFor(() => {
      expect(h.openSignInSheet).toHaveBeenCalled()
    })
    const request = sheetRequest()
    expect(request.endpoint.address).toBe('smb://Testnas/testshare')
    expect(request.refusal).toBe('authentication_rejected')
    expect(target.querySelector('.login-container'), 'no form renders in the pane').toBeNull()

    await unmount(component)
  })

  it('waits for the sign-in calmly behind the sheet, instead of saying the mount failed', async () => {
    // ❗ A share that wants an account is a normal step, not a failure: no Try again,
    // which without an account gets the same answer (cmdr-reports#10).
    h.openSignInSheet.mockReturnValue(new Promise(() => {}))
    h.mountNetworkShare.mockRejectedValue(refused(authRequired))
    const { target, component } = await mountViewAndActivateShare()

    await vi.waitFor(() => {
      expect(target.querySelector('.remote-connect .title')?.textContent.trim()).toBe('Sign in to open “testshare”')
    })
    expect(buttonLabels(target)).toEqual(['Sign in…', 'Go back'])

    await unmount(component)
  })

  it('words a refusal in the pane from the catalog, naming the host by its own name', async () => {
    // ERR-SHUSC: the pane showed the backend's English sentence in an otherwise Hungarian
    // UI. The words are the catalog's, and the server is the host a person picked.
    const notFound: MountError = { type: 'share_not_found', server: ADDRESS, share: 'testshare' }
    h.mountNetworkShare.mockRejectedValue(refused(notFound))
    const { target, component } = await mountViewAndActivateShare()

    await vi.waitFor(() => {
      expect(target.querySelector('.remote-connect .title')?.textContent).toBe(renderMountError(notFound, 'Testnas'))
    })
    expect(h.openSignInSheet, 'a share refusal asks for no credential').not.toHaveBeenCalled()

    await unmount(component)
  })

  it('Try again mounts again, and the refusal goes once it went through', async () => {
    h.mountNetworkShare
      .mockRejectedValueOnce(refused({ type: 'mount_missing', server: ADDRESS, share: 'testshare' }))
      .mockResolvedValueOnce({ mountPath: '/Volumes/testshare', alreadyMounted: false })
    const { target, component } = await mountViewAndActivateShare()

    await vi.waitFor(() => {
      expect(buttonLabels(target)).toContain('Try again')
    })
    button(target, 'Try again').click()

    await vi.waitFor(() => {
      expect(h.mountNetworkShare).toHaveBeenCalledTimes(2)
    })
    await vi.waitFor(() => {
      expect(target.querySelector('.remote-connect'), 'the retry went through').toBeNull()
    })

    await unmount(component)
  })

  it('goes back to the share list when the sign-in is cancelled', async () => {
    h.mountNetworkShare.mockRejectedValue(refused(authFailed))
    const { target, component } = await mountViewAndActivateShare()

    await vi.waitFor(() => {
      expect(h.openSignInSheet).toHaveBeenCalled()
    })
    await vi.waitFor(() => {
      expect(target.querySelector('.share-row'), 'the share list is where cancelling lands').toBeTruthy()
    })

    await unmount(component)
  })

  it('Go back clears the MCP mirror of the refusal and shows the share list again', async () => {
    h.mountNetworkShare.mockRejectedValue(refused(unreachable))
    const { target, component } = await mountViewAndActivateShare()

    await vi.waitFor(() => {
      expect(h.updateLeftPaneState).toHaveBeenCalledWith(
        expect.objectContaining({
          mountError: {
            share: 'testshare',
            reason: 'host_unreachable',
            message: renderMountError(unreachable, 'Testnas'),
          },
        }),
      )
    })
    button(target, 'Go back').click()

    await vi.waitFor(() => {
      expect(h.updateLeftPaneState).toHaveBeenCalledWith(expect.objectContaining({ mountError: null }))
    })
    await vi.waitFor(() => {
      expect(target.querySelector('.share-row')).toBeTruthy()
    })

    await unmount(component)
  })

  it('answers the keyboard in the refusal, one step back to the share list', async () => {
    // The refusal mounts neither browser, so the delegation below it landed on nothing:
    // every key went dead, and the MCP `nav_to_parent` tool (a synthetic Backspace) acked
    // `OK` while the pane sat still.
    h.mountNetworkShare.mockRejectedValue(refused(unreachable))
    const { target, component, api } = await mountViewAndActivateShare()
    await vi.waitFor(() => {
      expect(target.querySelector('.remote-connect')).toBeTruthy()
    })

    const event = new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true })
    api.handleKeyDown(event)
    await tick()

    expect(event.defaultPrevented, 'the key must be consumed here').toBe(true)
    // ONE step: back to this host's share list, not out to the host list.
    await vi.waitFor(() => {
      expect(target.querySelector('.share-row')).toBeTruthy()
    })

    await unmount(component)
  })

  it('counts no rows while the refusal is up, so a cursor move is refused instead of faked', async () => {
    h.mountNetworkShare.mockRejectedValue(refused(unreachable))
    const { target, component, api } = await mountViewAndActivateShare()

    await vi.waitFor(() => {
      expect(target.querySelector('.remote-connect')).toBeTruthy()
    })
    expect(api.getItemCount()).toBe(0)

    await unmount(component)
  })

  it('counts the rows on offer while the share list is up: `..` and the share', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    const component = mount(NetworkMountView, {
      target,
      props: { paneId: 'left', isFocused: true, initialNetworkHost: host },
    })
    const api = component as unknown as NetworkMountViewApi

    await vi.waitFor(() => {
      expect(target.querySelector('.share-row')).toBeTruthy()
    })
    expect(api.getItemCount()).toBe(2)

    await unmount(component)
  })
})

/** #604: the wait renders in the shared connecting view, with a Cancel that works. */
describe('NetworkMountView: a mount that waits', () => {
  /** Settles the running mount the way the backend answers a cancel. */
  let answerMount: (error: MountError) => void = () => {}

  beforeEach(() => {
    vi.clearAllMocks()
    document.body.innerHTML = ''
    h.fetchShares.mockResolvedValue({ shares: [testshare], authMode: 'guest_allowed', fromCache: false })
    h.getSmbCredentials.mockRejectedValue(new Error('not found'))
    h.mountNetworkShare.mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          answerMount = (error) => {
            reject(refused(error))
          }
        }),
    )
  })

  async function waitForTheWait(target: HTMLElement) {
    await vi.waitFor(
      () => {
        expect(target.querySelector('.remote-connect .content')).toBeTruthy()
      },
      { timeout: 3000 },
    )
  }

  it('offers Cancel while it waits, and Cancel calls off that mount and goes back to the share list', async () => {
    const { target, component } = await mountViewAndActivateShare()
    await waitForTheWait(target)

    button(target, 'Cancel').click()
    expect(h.cancelServerConnect).toHaveBeenCalledWith('server-connect-test-attempt')

    answerMount({ type: 'cancelled', share: 'testshare' })
    await vi.waitFor(() => {
      expect(target.querySelector('.share-row')).toBeTruthy()
    })
    expect(target.querySelector('.remote-connect'), 'a cancel the person asked for is no refusal').toBeNull()

    await unmount(component)
  })

  it('names the share it mounts, its server, and the address it dials', async () => {
    const { target, component } = await mountViewAndActivateShare()
    await waitForTheWait(target)

    expect(must(target.querySelector('.remote-connect .title'), 'the heading').textContent).toContain(
      'Connecting to testshare',
    )
    expect(must(target.querySelector('.remote-connect .place-name'), 'the identity line').textContent).toBe(
      'testshare on Testnas',
    )
    expect(must(target.querySelector('.remote-connect .place-address'), 'the address').textContent).toBe(
      `${ADDRESS}:445`,
    )

    await unmount(component)
  })

  it('explains a stuck sign-in helper in the pane, with the command that restarts it', async () => {
    const { target, component } = await mountViewAndActivateShare()
    await waitForTheWait(target)

    answerMount({ type: 'sign_in_helper_stuck', server: ADDRESS })
    await vi.waitFor(() => {
      expect(target.querySelector('.remote-connect .title')?.textContent).toContain('killall NetAuthSysAgent')
    })

    await unmount(component)
  })
})

/**
 * ❗ cmdr-reports#7: "Cmdr tried to open the share on that server." A share queued
 * to auto-mount (an `smb://host/share` address, "Copy path between panes") is a
 * one-shot for the visit it was queued with.
 */
describe('NetworkMountView auto-mount', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    document.body.innerHTML = ''
    h.hosts = [host]
    h.fetchShares.mockResolvedValue({ shares: [testshare], authMode: 'guest_allowed', fromCache: false })
    h.getSmbCredentials.mockRejectedValue(new Error('not found'))
    h.resolvePathVolume.mockResolvedValue({ volume: null })
    h.mountNetworkShare.mockResolvedValue({ mountPath: '/Volumes/testshare', alreadyMounted: false })
  })

  function mountQueued(consumed: () => void) {
    const target = document.createElement('div')
    document.body.appendChild(target)
    const component = mount(NetworkMountView, {
      target,
      props: {
        paneId: 'left',
        isFocused: true,
        initialNetworkHost: host,
        initialAutoMountShare: 'testshare',
        onAutoMountConsumed: consumed,
      },
    })
    return { target, component, api: component as unknown as NetworkMountViewApi }
  }

  it('mounts a queued share once, and opening the host again later only lists its shares', async () => {
    const consumed = vi.fn()
    const { target, component, api } = mountQueued(consumed)

    await vi.waitFor(() => {
      expect(h.mountNetworkShare).toHaveBeenCalledTimes(1)
    })
    // The mount went through, so the pane is back on the servers list.
    await vi.waitFor(() => {
      expect(target.querySelector('.server-row')).toBeTruthy()
    })
    expect(consumed).toHaveBeenCalled()

    // The person opens the same host from the list, to browse it.
    api.openCursorItem()
    await vi.waitFor(() => {
      expect(target.querySelector('.share-row')).toBeTruthy()
    })
    await tick()

    expect(h.mountNetworkShare, 'no second mount nobody asked for').toHaveBeenCalledTimes(1)
    await unmount(component)
  })

  it('doesn’t dial the queued share again once the person cancelled it', async () => {
    // The wait replaces the share list, and Cancel brings a FRESH one back: a queue that
    // was spent only on success fired again on it, so Cancel re-dialed at once.
    let answerMount: (error: MountError) => void = () => {}
    h.mountNetworkShare.mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          answerMount = (error) => {
            reject(refused(error))
          }
        }),
    )
    const { target, component } = mountQueued(vi.fn())
    await vi.waitFor(
      () => {
        expect(target.querySelector('.remote-connect .content')).toBeTruthy()
      },
      { timeout: 3000 },
    )

    button(target, 'Cancel').click()
    answerMount({ type: 'cancelled', share: 'testshare' })
    await vi.waitFor(() => {
      expect(target.querySelector('.share-row')).toBeTruthy()
    })
    await tick()

    expect(h.mountNetworkShare, 'Cancel means not now').toHaveBeenCalledTimes(1)
    await unmount(component)
  })
})

/**
 * ❗ Back from a host's share list puts the cursor on THAT host's row, found by the
 * host's identity: it used to land on the first row whichever host the person came from
 * (final QA).
 */
describe('NetworkMountView: back from a share list', () => {
  const attic: NetworkHost = { id: 'attic-id', name: 'Attic', hostname: 'attic.local', port: 445, source: 'discovered' }

  beforeEach(() => {
    vi.clearAllMocks()
    document.body.innerHTML = ''
    h.hosts = [attic, host]
    h.fetchShares.mockResolvedValue({ shares: [testshare], authMode: 'guest_allowed', fromCache: false })
    h.getSmbCredentials.mockRejectedValue(new Error('not found'))
  })

  it('puts the cursor on the host it came back from', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    const component = mount(NetworkMountView, { target, props: { paneId: 'left', isFocused: true } })
    const api = component as unknown as NetworkMountViewApi
    await vi.waitFor(() => {
      expect(target.querySelectorAll('.server-row').length).toBeGreaterThan(1)
    })

    // Down to Testnas (the second row), then in.
    api.handleKeyDown(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }))
    await tick()
    api.openCursorItem()
    await vi.waitFor(() => {
      expect(target.querySelector('.share-row')).toBeTruthy()
    })

    api.handleKeyDown(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true }))
    await vi.waitFor(() => {
      expect(target.querySelector('.server-row.is-under-cursor')?.textContent).toContain('Testnas')
    })
    await unmount(component)
  })
})
