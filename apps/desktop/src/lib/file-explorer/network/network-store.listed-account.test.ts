/**
 * Which account a host's share list signed in as: what the Servers list and the
 * share list's header say ("as guest", "as testuser"). ❗ Only from the listings
 * themselves: nothing here reads the Keychain.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const { ipc } = vi.hoisted(() => ({
  ipc: {
    listSharesOnHost: vi.fn(),
  },
}))
vi.mock('#lib/tauri-commands/index.js', () => ipc)
vi.mock('#lib/settings/network-settings.js', () => ({
  getNetworkTimeoutMs: () => 1000,
  getShareCacheTtlMs: () => 1000,
}))

import {
  fetchShares,
  forgetShareListsOfMachine,
  getListedAccount,
  guestListingWorked,
  setListedAccount,
} from './network-store.svelte'
import type { NetworkHost } from '../types'

const host = (id: string): NetworkHost => ({ id, name: id, hostname: 'localhost', port: 11482, source: 'manual' })
const listing = (authMode: string) => ({ shares: [], authMode, fromCache: false })

beforeEach(() => {
  vi.clearAllMocks()
})

describe('the account a share list signed in as', () => {
  it('is guest after a guest listing', async () => {
    ipc.listSharesOnHost.mockResolvedValue(listing('guest_allowed'))
    await fetchShares(host('a'))
    expect(getListedAccount('a')).toEqual({ kind: 'guest' })
  })

  it('is the account a sign-in listed with, and a later refresh from the cache keeps it', async () => {
    setListedAccount('b', { kind: 'user', username: 'testuser' })
    ipc.listSharesOnHost.mockResolvedValue(listing('creds_required'))
    await fetchShares(host('b'))
    expect(getListedAccount('b')).toEqual({ kind: 'user', username: 'testuser' })
  })

  /**
   * ❗ A background listing never takes a known account back to guest: "Sign in as
   * testuser" reverted to "as guest" three seconds later, when the prefetch on the way
   * back to the Servers list listed as guest (final QA).
   */
  it('stays the account a sign-in chose when a later listing comes back as guest', async () => {
    setListedAccount('e', { kind: 'user', username: 'testuser' })
    ipc.listSharesOnHost.mockResolvedValue(listing('guest_allowed'))
    await fetchShares(host('e'))
    expect(getListedAccount('e')).toEqual({ kind: 'user', username: 'testuser' })
    expect(guestListingWorked('e'), 'and it knows guest works there, for "Browse as guest"').toBe(true)
  })

  it('is unknown after a listing that failed, and after the machine’s lists are dropped', async () => {
    setListedAccount('c', { kind: 'guest' })
    ipc.listSharesOnHost.mockRejectedValue({ type: 'auth_required', message: 'sign in' })
    await fetchShares(host('c')).catch(() => {})
    expect(getListedAccount('c')).toBeUndefined()

    setListedAccount('d', { kind: 'guest' })
    forgetShareListsOfMachine(host('d'))
    expect(getListedAccount('d')).toBeUndefined()
  })
})
