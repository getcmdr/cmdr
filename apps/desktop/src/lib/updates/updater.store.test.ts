/**
 * A copy installed from the Microsoft Store. The Store updates it and its files are read-only, so the backend answers
 * `updatedByStore` without a request, and the app says the Store keeps it up to date: no failure, no download, no
 * "up to date" claim of its own, and no more background checks.
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

describe('a copy the Microsoft Store updates', () => {
  beforeEach(() => {
    _resetUpdaterStateForTest()
    vi.clearAllMocks()
    checkForUpdateMock.mockResolvedValue({ kind: 'updatedByStore' })
  })

  afterEach(() => {
    _resetUpdaterStateForTest()
  })

  it('says the Store keeps it up to date, never that Cmdr found no updates', async () => {
    await checkForUpdates('settings')

    expect(updateState.status).toBe('idle')
    expect(updateState.failure).toBeNull()
    expect(updateState.updatedByStore).toBe(true)
    expect(formatUpdateStatus(updateState)).toBe('The Microsoft Store keeps this copy of Cmdr up to date.')
  })

  it('downloads nothing, logs nothing above info, and reports its own outcome', async () => {
    await checkForUpdates('startup')

    expect(downloadUpdateMock).not.toHaveBeenCalled()
    expect(logger.warn).not.toHaveBeenCalled()
    expect(logger.error).not.toHaveBeenCalled()
    expect(trackEventMock).toHaveBeenCalledWith(
      'update_check',
      expect.objectContaining({ outcome: 'updated_by_store', failure: 'none' }),
    )
  })

  it('answers the same on a later check a person asks for', async () => {
    await checkForUpdates('startup')
    await checkForUpdates('command')

    expect(checkForUpdateMock).toHaveBeenCalledTimes(2)
    expect(updateState.updatedByStore).toBe(true)
    expect(formatUpdateStatus(updateState)).toBe('The Microsoft Store keeps this copy of Cmdr up to date.')
  })
})
