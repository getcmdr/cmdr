/**
 * Behavior tests for NetworkMountView, the Servers hub: every way it opens a server's
 * share list goes to the pane (`onOpenServer`), which lists it as a real listing and
 * mounts a share (`share-list-pane.svelte.ts`, `share-mount.svelte.ts`); and the hub lands
 * its cursor on the server the pane came back from.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, unmount, tick } from 'svelte'
import NetworkMountView from './NetworkMountView.svelte'
import type { NetworkHost } from '../types'
import { explorerState } from './explorer-state.svelte'
import { createInitialTabState } from './tab-operations'
import { createTabManager, getActiveTab } from '../tabs/tab-state-manager.svelte'

const h = vi.hoisted(() => ({
  fetchShares: vi.fn(),
  mountNetworkShare: vi.fn(),
  cancelServerConnect: vi.fn(() => Promise.resolve(true)),
  saveSmbCredentials: vi.fn(),
  setSmbAccountPreference: vi.fn(() => Promise.resolve(true)),
  resolvePathVolume: vi.fn(),
  updateLeftPaneState: vi.fn(() => Promise.resolve()),
  openSignInSheet: vi.fn(),
  warn: vi.fn(),
  hosts: [] as NetworkHost[],
  listSavedServers: vi.fn((): Promise<unknown[]> => Promise.resolve([])),
}))

vi.mock('#lib/tauri-commands/index.js', () => ({
  DEFAULT_VOLUME_ID: 'root',
  mountNetworkShare: h.mountNetworkShare,
  cancelServerConnect: h.cancelServerConnect,
  newServerAttemptId: () => 'server-connect-test-attempt',
  resolvePathVolume: h.resolvePathVolume,
  saveSmbCredentials: h.saveSmbCredentials,
  setSmbAccountPreference: h.setSmbAccountPreference,
  listSharesWithCredentials: vi.fn(() => Promise.resolve({ shares: [], authMode: 'unknown', fromCache: false })),
  isUsingCredentialFileFallback: vi.fn(() => Promise.resolve(false)),
  updateKnownShare: vi.fn(() => Promise.resolve()),
  getUsernameHint: vi.fn(() => Promise.resolve(null)),
  getKnownShareByName: vi.fn(() => Promise.resolve(null)),
  updateLeftPaneState: h.updateLeftPaneState,
  updateRightPaneState: vi.fn(() => Promise.resolve()),
  disconnectNetworkHost: vi.fn(() => Promise.resolve()),
  connectToServer: vi.fn(() => Promise.resolve()),
  listSavedServers: h.listSavedServers,
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
  forgetCredentials: vi.fn(() => Promise.resolve()),
}))

vi.mock('#lib/utils/confirm-dialog.js', () => ({ confirmDialog: vi.fn(() => Promise.resolve(false)) }))
vi.mock('#lib/ui/toast/index.js', () => ({ addToast: vi.fn(() => 'id') }))
vi.mock('../network/lazy-trigger', () => ({ triggerNetworkDiscovery: vi.fn() }))

const host: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'Testnas.local',
  ipAddress: '192.168.50.10',
  port: 445,
  source: 'discovered',
}

/** The exported NetworkMountView API surface the tests drive. */
interface NetworkMountViewApi {
  openCursorItem: () => void
  handleKeyDown: (e: KeyboardEvent) => void
}

describe('NetworkMountView: opening a server', () => {
  const attic: NetworkHost = { id: 'attic-id', name: 'Attic', hostname: 'attic.local', port: 445, source: 'discovered' }

  beforeEach(() => {
    vi.clearAllMocks()
    document.body.innerHTML = ''
    h.hosts = [attic, host]
  })

  it('asks the pane to open the server under the cursor as its share list, and lists nothing itself', async () => {
    const onOpenServer = vi.fn()
    const target = document.createElement('div')
    document.body.appendChild(target)
    const component = mount(NetworkMountView, { target, props: { paneId: 'left', isFocused: true, onOpenServer } })
    const api = component as unknown as NetworkMountViewApi
    await vi.waitFor(() => {
      expect(target.querySelectorAll('.server-row').length).toBeGreaterThan(1)
    })

    // Down to Testnas (the second row), then in.
    api.handleKeyDown(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }))
    await tick()
    api.openCursorItem()

    expect(onOpenServer).toHaveBeenCalledWith(host, {})
    // #324: the hub doesn't list it; the pane's listing does.
    expect(h.fetchShares).not.toHaveBeenCalled()
    await unmount(component)
  })
})

describe('NetworkMountView: back from a share list', () => {
  const attic: NetworkHost = { id: 'attic-id', name: 'Attic', hostname: 'attic.local', port: 445, source: 'discovered' }

  beforeEach(() => {
    vi.clearAllMocks()
    document.body.innerHTML = ''
    h.hosts = [attic, host]
  })

  it("lands on the server the tab's hub landing names, once", async () => {
    // What `..` from a share list (or a restored share-list tab) leaves on the tab.
    explorerState.setTabMgr('left', createTabManager(createInitialTabState('smb://', 'network')))
    getActiveTab(explorerState.getTabMgr('left')).landOnServer = 'smb://testnas'
    const target = document.createElement('div')
    document.body.appendChild(target)
    const component = mount(NetworkMountView, { target, props: { paneId: 'left', isFocused: true } })

    await vi.waitFor(() => {
      expect(target.querySelector('.server-row.is-under-cursor')?.textContent).toContain('Testnas')
    })
    expect(getActiveTab(explorerState.getTabMgr('left')).landOnServer).toBeUndefined()
    await unmount(component)
  })

  /** A server the person has a share of lists as a saved row merged with its nearby host. */
  it('lands on a saved server that discovery also sees', async () => {
    h.listSavedServers.mockImplementation(() =>
      Promise.resolve([
        {
          id: 'manual-testnas-445',
          protocol: 'smb',
          displayName: 'Testnas',
          nameSource: 'fallback',
          address: 'Testnas',
          username: null,
          pinned: false,
          lastConnectedAt: null,
          autoReconnect: null,
          places: [],
          awaitingFirstConnect: false,
        },
      ]),
    )
    try {
      explorerState.setTabMgr('left', createTabManager(createInitialTabState('smb://', 'network')))
      getActiveTab(explorerState.getTabMgr('left')).landOnServer = 'smb://testnas'
      const target = document.createElement('div')
      document.body.appendChild(target)
      const component = mount(NetworkMountView, { target, props: { paneId: 'left', isFocused: true } })
      await vi.waitFor(() => {
        expect(target.querySelector('.server-row.is-under-cursor')?.textContent).toContain('Testnas')
      })
      await unmount(component)
    } finally {
      h.listSavedServers.mockImplementation(() => Promise.resolve([]))
    }
  })

  it('keeps the cursor where it would be when the landing names a server it doesn’t list', async () => {
    explorerState.setTabMgr('left', createTabManager(createInitialTabState('smb://', 'network')))
    getActiveTab(explorerState.getTabMgr('left')).landOnServer = 'smb://gone-nas'
    const target = document.createElement('div')
    document.body.appendChild(target)
    const component = mount(NetworkMountView, { target, props: { paneId: 'left', isFocused: true } })

    await vi.waitFor(() => {
      expect(target.querySelector('.server-row.is-under-cursor')?.textContent).toContain('Attic')
    })
    await unmount(component)
  })
})
