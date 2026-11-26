/**
 * Which server a share-list path names, for its label and every action on it. Discovery's
 * hosts name most; the server a mounted share's `..` leads to (`share-parent-row.svelte.ts`)
 * names the rest: a share macOS mounted by an address discovery never saw (a Tailscale IP)
 * still has a share list to mount from and sign in to, under the name `..` gave it.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { NetworkHost } from '../types'

const h = vi.hoisted(() => ({ hosts: [] as NetworkHost[] }))
vi.mock('./network-store.svelte', () => ({
  getNetworkHosts: () => h.hosts,
  fetchShares: vi.fn(() => Promise.resolve()),
  setListedAccount: vi.fn(),
}))
vi.mock('#lib/tauri-commands/index.js', () => ({ getSmbShareParent: vi.fn(), setSmbShareParentRow: vi.fn() }))
vi.mock('#lib/settings/reactive-settings.svelte.js', () => ({ getShowShareParentRow: () => true }))
vi.mock('#lib/settings/index.js', () => ({ getSetting: () => true, setSetting: vi.fn() }))

import { shareListServerLabel, shareListServerOf } from './share-list-label'
import { noteShareParent } from './share-parent-row.svelte'

const bonjour: NetworkHost = {
  id: 'testnas',
  name: 'Testnas',
  hostname: 'testnas.local',
  ipAddress: '198.51.100.7',
  port: 445,
  source: 'discovered',
}

/** The host Rust makes for a server only a mount knows (`smb_share_parent::made_host`). */
const mountedOnly: NetworkHost = {
  id: 'manual-100-64-0-9-445',
  name: '100.64.0.9',
  hostname: '100.64.0.9',
  ipAddress: '100.64.0.9',
  port: 445,
  source: 'manual',
}

beforeEach(() => {
  h.hosts = [bonjour]
})

describe('shareListServerLabel', () => {
  it('names a discovered server by its Bonjour name', () => {
    expect(shareListServerLabel('smb://testnas')).toBe('Testnas')
  })

  it('names a server only a mounted share knows by the name `..` out of it gave', () => {
    noteShareParent('smb-test-label-typed', {
      host: { ...mountedOnly, id: 'manual-100-64-0-8-445', name: '100.64.0.8', hostname: '100.64.0.8' },
      serverLabel: 'Attic NAS',
      shareName: 'media',
      showParentRow: null,
    })
    expect(shareListServerLabel('smb://100.64.0.8')).toBe('Attic NAS')
  })

  it('falls back to the path’s own key for a server nothing knows', () => {
    expect(shareListServerLabel('smb://nobody.test')).toBe('nobody.test')
  })
})

describe('shareListServerOf', () => {
  it('finds a server only a mounted share knows, so its shares mount and its sign-in has a host', () => {
    noteShareParent('smb-test-label-mounted', {
      host: mountedOnly,
      serverLabel: '100.64.0.9',
      shareName: 'media',
      showParentRow: null,
    })
    expect(shareListServerOf('smb://100.64.0.9')).toEqual({ host: mountedOnly, label: '100.64.0.9' })
  })

  it('prefers the discovered host, whose id the hub caches shares and sign-in under', () => {
    noteShareParent('smb-test-label-both', {
      host: { ...bonjour, id: 'made-elsewhere' },
      serverLabel: 'Testnas',
      shareName: 'media',
      showParentRow: null,
    })
    expect(shareListServerOf('smb://testnas')?.host.id).toBe('testnas')
  })
})
