/**
 * `noteCachedCredentials`: what the share list's "Forget saved password" button
 * knows. ❗ It asks the backend's in-memory cache only, and never reads the
 * Keychain: each access can raise a system prompt (`CLAUDE.md` § "Never ask the
 * Keychain twice").
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const { ipc } = vi.hoisted(() => ({
  ipc: {
    hasCachedSmbCredentials: vi.fn(),
    hasSmbCredentials: vi.fn(),
  },
}))
vi.mock('#lib/tauri-commands/index.js', () => ipc)

import { getCredentialStatus, noteCachedCredentials } from './network-store.svelte'

describe('noteCachedCredentials', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('marks a host whose password this session already read', async () => {
    ipc.hasCachedSmbCredentials.mockResolvedValue(true)
    await noteCachedCredentials('localhost:11482')
    expect(getCredentialStatus('LOCALHOST:11482')).toBe('has_creds')
  })

  it('leaves an unread host as it was, and never touches the Keychain', async () => {
    ipc.hasCachedSmbCredentials.mockResolvedValue(false)
    await noteCachedCredentials('localhost:11480')
    expect(getCredentialStatus('localhost:11480')).toBe('unknown')
    expect(ipc.hasSmbCredentials).not.toHaveBeenCalled()
  })
})
