/**
 * A newer release whose `latest.json` carries no build for this install's platform. The check used to answer
 * "up to date", which told a stranded install it was current; the backend now says `noBuildForThisPlatform`, and the
 * app says there's a newer Cmdr, but not for this system yet: no failure, no error report, no version it can't install.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { checkForUpdateMock, downloadUpdateMock, trackEventMock, logger } = vi.hoisted(() => ({
  checkForUpdateMock: vi.fn(),
  downloadUpdateMock: vi.fn(() => Promise.resolve()),
  trackEventMock: vi.fn(),
  logger: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

vi.mock('#lib/tauri-commands/index.js', () => ({
  checkForUpdate: checkForUpdateMock,
  downloadUpdate: downloadUpdateMock,
  installUpdate: vi.fn(() => Promise.resolve({ securityNotice: null })),
  updateWriteBlocker: vi.fn(() => Promise.resolve(null)),
  updateCheckDueIn: vi.fn(() => Promise.resolve(0)),
  recordUpdateCheck: vi.fn(() => Promise.resolve()),
  trackEvent: trackEventMock,
}))

vi.mock('#lib/ui/toast/index.js', () => ({ addToast: vi.fn(), dismissToast: vi.fn() }))

vi.mock('#lib/settings/settings-store.js', () => ({
  getSetting: vi.fn((id: string) => (id === 'onboarding.completed' ? true : 60 * 60 * 1000)),
  setSetting: vi.fn(),
  forceSave: vi.fn(() => Promise.resolve(true)),
  onSpecificSettingChange: vi.fn(() => () => {}),
}))

vi.mock('@tauri-apps/api/app', () => ({ getVersion: vi.fn(() => Promise.resolve('0.28.0')) }))

vi.mock('#lib/logging/logger.js', () => ({ getAppLogger: () => logger }))

import { _resetUpdaterStateForTest, checkForUpdates, updateState } from './updater.svelte'
import { formatUpdateStatus } from './update-status-text'

describe('a newer release with no build for this platform', () => {
  beforeEach(() => {
    _resetUpdaterStateForTest()
    vi.clearAllMocks()
    checkForUpdateMock.mockResolvedValue({ kind: 'noBuildForThisPlatform' })
  })

  afterEach(() => {
    _resetUpdaterStateForTest()
  })

  it('says there’s a newer Cmdr but not for this system yet, never that it’s up to date', async () => {
    await checkForUpdates('command')

    expect(updateState.status).toBe('idle')
    expect(updateState.failure).toBeNull()
    expect(updateState.noBuild).toBe(true)
    expect(formatUpdateStatus(updateState)).toBe(
      'There’s a newer Cmdr, but not for this system yet. Check again later.',
    )
  })

  it('downloads nothing, logs nothing above info, and reports its own outcome', async () => {
    await checkForUpdates('poll')

    expect(downloadUpdateMock).not.toHaveBeenCalled()
    expect(logger.warn).not.toHaveBeenCalled()
    expect(logger.error).not.toHaveBeenCalled()
    expect(trackEventMock).toHaveBeenCalledWith(
      'update_check',
      expect.objectContaining({ outcome: 'no_build_for_platform', failure: 'none' }),
    )
  })

  it('clears once a later check finds this platform current again', async () => {
    await checkForUpdates('poll')
    checkForUpdateMock.mockResolvedValueOnce({ kind: 'upToDate' })

    await checkForUpdates('poll')

    expect(updateState.noBuild).toBe(false)
    expect(formatUpdateStatus(updateState)).toBe('No updates found. Current version: v0.28.0')
  })
})
