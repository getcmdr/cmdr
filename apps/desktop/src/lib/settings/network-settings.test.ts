import { describe, it, expect, vi, beforeEach } from 'vitest'
import { getNetworkTimeoutMs, getMountTimeoutMs, getShareCacheTtlMs } from './network-settings'

vi.mock('#lib/settings/settings-store.js', () => ({
  getSetting: vi.fn(),
}))

import { getSetting } from './settings-store'

const mockGetSetting = vi.mocked(getSetting)

describe('getNetworkTimeoutMs', () => {
  beforeEach(() => {
    vi.resetAllMocks()
  })

  it('returns the stored timeout, which is already in milliseconds', () => {
    mockGetSetting.mockImplementation((key: string) => (key === 'network.timeout' ? 45_000 : ''))
    expect(getNetworkTimeoutMs()).toBe(45_000)
  })
})

describe('getMountTimeoutMs', () => {
  beforeEach(() => {
    vi.resetAllMocks()
  })

  it('returns the mount timeout from settings', () => {
    mockGetSetting.mockReturnValue(30_000)
    expect(getMountTimeoutMs()).toBe(30_000)
  })
})

describe('getShareCacheTtlMs', () => {
  beforeEach(() => {
    vi.resetAllMocks()
  })

  it('returns the share cache duration from settings', () => {
    mockGetSetting.mockReturnValue(60_000)
    expect(getShareCacheTtlMs()).toBe(60_000)
  })
})
