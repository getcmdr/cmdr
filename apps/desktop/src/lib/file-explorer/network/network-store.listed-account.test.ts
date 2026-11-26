/**
 * Which account a host's share list signed in as: what the Servers list and the
 * share list's header say ("as guest", "as testuser"). ❗ Only from the listings
 * themselves (`listing-complete`'s account, a sheet's listing): nothing here reads the
 * Keychain or lists anything. Plus the host the backend resolved a share list to, for a
 * server discovery doesn't list.
 */
import { describe, it, expect, vi } from 'vitest'

vi.mock('#lib/tauri-commands/index.js', () => ({}))

import {
  forgetShareListsOfMachine,
  getListedAccount,
  getResolvedShareListHost,
  guestListingWorked,
  noteResolvedShareListHost,
  setListedAccount,
} from './network-store.svelte'
import type { NetworkHost } from '../types'

const host = (id: string): NetworkHost => ({ id, name: id, hostname: 'localhost', port: 11482, source: 'manual' })

describe('the account a share list signed in as', () => {
  it('is guest after a guest listing, which also says guest works there', () => {
    setListedAccount('a', { kind: 'guest' })
    expect(getListedAccount('a')).toEqual({ kind: 'guest' })
    expect(guestListingWorked('a'), 'for "Browse as guest"').toBe(true)
  })

  it('is the account a sign-in listed with, and guest stays unproven there', () => {
    setListedAccount('b', { kind: 'user', username: 'testuser' })
    expect(getListedAccount('b')).toEqual({ kind: 'user', username: 'testuser' })
    expect(guestListingWorked('b')).toBe(false)
  })

  it('is unknown after the machine’s lists are dropped', () => {
    setListedAccount('d', { kind: 'guest' })
    forgetShareListsOfMachine(host('d'))
    expect(getListedAccount('d')).toBeUndefined()
  })
})

describe('the host the backend resolved a share list to', () => {
  it('is found again under any spelling of the same share list, and never for another', () => {
    const saved: NetworkHost = { ...host('manual-TestNAS-local-445'), name: 'TestNAS.local', port: 445 }
    noteResolvedShareListHost('smb://testnas.local', saved)
    expect(getResolvedShareListHost('smb://TestNAS.local:445/')).toBe(saved)
    expect(getResolvedShareListHost('smb://testnas.local:4450')).toBeUndefined()
    expect(getResolvedShareListHost('smb://')).toBeUndefined()
  })
})
