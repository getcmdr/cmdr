/**
 * Tier 3 a11y tests for `NetworkMountView.svelte`, the Servers hub. A server's share list
 * is a file listing (`full-list.a11y.test.ts`), and ServersHub has its own a11y tests, so
 * this audits the shell.
 */

import { describe, it, vi } from 'vitest'
import { mount, tick } from 'svelte'
import NetworkMountView from './NetworkMountView.svelte'
import { expectNoA11yViolations } from '#lib/test-a11y.js'

vi.mock('#lib/tauri-commands/index.js', () => ({
  DEFAULT_VOLUME_ID: 'root',
  mountNetworkShare: vi.fn(() => Promise.resolve({ mountPath: '/Volumes/Public' })),
  noteNetworkAction: vi.fn(() => Promise.resolve()),
  setServersViewShown: vi.fn(() => Promise.resolve()),
  resolvePathVolume: vi.fn(() => Promise.resolve({ volume: null })),
  updateLeftPaneState: vi.fn(() => Promise.resolve()),
  updateRightPaneState: vi.fn(() => Promise.resolve()),
  disconnectNetworkHost: vi.fn(() => Promise.resolve()),
  listSharesWithCredentials: vi.fn(() => Promise.resolve([])),
  saveSmbCredentials: vi.fn(() => Promise.resolve()),
  isUsingCredentialFileFallback: vi.fn(() => Promise.resolve(false)),
  updateKnownShare: vi.fn(() => Promise.resolve()),
  getUsernameHint: vi.fn(() => Promise.resolve(null)),
  getKnownShareByName: vi.fn(() => Promise.resolve(null)),
  connectToServer: vi.fn(() => Promise.resolve()),
  notifyDialogOpened: vi.fn(() => Promise.resolve()),
  notifyDialogClosed: vi.fn(() => Promise.resolve()),
}))

vi.mock('#lib/settings/network-settings.js', () => ({
  getMountTimeoutMs: () => 15000,
  getNetworkTimeoutMs: () => 5000,
  getShareCacheTtlMs: () => 300000,
}))

vi.mock('#lib/logging/logger.js', () => ({
  getAppLogger: () => ({
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  }),
}))

vi.mock('../network/network-store.svelte', () => ({
  getNetworkHosts: () => [],
  getDiscoveryState: () => 'idle',
  holdDiscoveryForServersView: () => () => {},
  isHostResolving: () => false,
  getListedAccount: () => null,
  noteCachedCredentials: vi.fn(() => Promise.resolve()),
  getShareState: () => undefined,
  getShareCount: () => null,
  isListingShares: () => false,
  isShareDataStale: () => false,
  refreshAllStaleShares: vi.fn(),
  clearShareState: vi.fn(),
  setShareState: vi.fn(),
  setCredentialStatus: vi.fn(),
  fetchShares: vi.fn(() => Promise.resolve()),
  getCredentialStatus: () => 'unknown',
  forgetCredentials: vi.fn(() => Promise.resolve()),
}))

vi.mock('#lib/utils/confirm-dialog.js', () => ({
  confirmDialog: vi.fn(() => Promise.resolve(false)),
}))

vi.mock('#lib/ui/toast/index.js', () => ({
  addToast: vi.fn(() => 'id'),
}))

describe('NetworkMountView a11y', () => {
  it('default (no host - list browser) has no a11y violations', async () => {
    const target = document.createElement('div')
    document.body.appendChild(target)
    mount(NetworkMountView, {
      target,
      props: {
        paneId: 'left',
        isFocused: true,
      },
    })
    await tick()
    await expectNoA11yViolations(target)
  })
})
