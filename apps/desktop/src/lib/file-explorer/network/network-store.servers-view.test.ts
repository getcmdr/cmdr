/**
 * `holdDiscoveryForServersView`: the backend browses mDNS only while a Servers view
 * is on screen, so the store tells it when the FIRST view appears and when the LAST
 * one leaves. Two panes can show the view at once. ❗ Holding it lists no server's shares (#324):
 * a share list is read when a pane opens it, and the Servers list itself never dials.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const { ipc } = vi.hoisted(() => ({
  ipc: {
    setServersViewShown: vi.fn(() => Promise.resolve()),
    listNetworkHosts: vi.fn(() =>
      Promise.resolve([{ id: 'testnas', name: 'Testnas', hostname: 'testnas.local', port: 445, source: 'manual' }]),
    ),
    getNetworkDiscoveryState: vi.fn(() => Promise.resolve('idle')),
    onNetworkHostFound: vi.fn(() => Promise.resolve(() => {})),
    onNetworkHostLost: vi.fn(() => Promise.resolve(() => {})),
    onNetworkHostResolved: vi.fn(() => Promise.resolve(() => {})),
    onNetworkDiscoveryStateChanged: vi.fn(() => Promise.resolve(() => {})),
    resolveNetworkHost: vi.fn(() => Promise.resolve(null)),
    listSharesOnHost: vi.fn(() => Promise.resolve({ shares: [], authMode: 'guest_allowed' })),
    listSavedServers: vi.fn(() => Promise.resolve([{ id: 'testnas', protocol: 'smb', places: [] }])),
  },
}))
vi.mock('#lib/tauri-commands/index.js', () => ipc)
vi.mock('#lib/settings/index.js', () => ({ initializeSettings: vi.fn(() => Promise.resolve()) }))

import { cleanupNetworkDiscovery, holdDiscoveryForServersView, initNetworkDiscovery } from './network-store.svelte'

describe('holdDiscoveryForServersView', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('asks for the browse when a view appears and lets it go when it leaves', () => {
    const release = holdDiscoveryForServersView()
    expect(ipc.setServersViewShown).toHaveBeenCalledWith(true)

    release()
    expect(ipc.setServersViewShown).toHaveBeenLastCalledWith(false)
  })

  it('keeps the browse while either of two panes still shows the view', () => {
    const releaseLeft = holdDiscoveryForServersView()
    const releaseRight = holdDiscoveryForServersView()
    releaseLeft()
    expect(ipc.setServersViewShown).toHaveBeenCalledTimes(1)
    expect(ipc.setServersViewShown).not.toHaveBeenCalledWith(false)

    releaseRight()
    expect(ipc.setServersViewShown).toHaveBeenLastCalledWith(false)
  })

  it('counts a double release once', () => {
    const releaseLeft = holdDiscoveryForServersView()
    const releaseRight = holdDiscoveryForServersView()
    releaseLeft()
    releaseLeft()
    expect(ipc.setServersViewShown).not.toHaveBeenCalledWith(false)
    releaseRight()
  })
})

describe('a Servers view on screen', () => {
  /** #324: the Servers list never dials. A saved, resolved SMB host is listed only when a pane opens it. */
  it('lists no host’s shares, not even a saved server’s', async () => {
    await initNetworkDiscovery()
    const release = holdDiscoveryForServersView()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(ipc.listSharesOnHost).not.toHaveBeenCalled()
    release()
    cleanupNetworkDiscovery()
  })
})
