/**
 * Tier 3 a11y tests for `NetworkMountView.svelte`.
 *
 * The component renders one of three inner views: ServersHub (no
 * host), PlacesBrowser (host selected), mounting spinner, or a mount
 * error state. Mounting/error states are deterministic and inline in
 * this file; ServersHub + PlacesBrowser have their own a11y tests,
 * so this audits the shell with no host.
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
  showNetworkHostContextMenu: vi.fn(() => Promise.resolve()),
  onNetworkHostContextAction: vi.fn(() => Promise.resolve(() => {})),
  disconnectNetworkHost: vi.fn(() => Promise.resolve()),
  listSharesWithCredentials: vi.fn(() => Promise.resolve([])),
  saveSmbCredentials: vi.fn(() => Promise.resolve()),
  getSmbCredentials: vi.fn(() => Promise.resolve(null)),
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
  checkCredentialsForHost: vi.fn(() => Promise.resolve()),
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
