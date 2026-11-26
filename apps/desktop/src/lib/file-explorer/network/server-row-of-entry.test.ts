/**
 * A Servers list row becomes the row every server action takes BY ITS OWN ID: the saved
 * entry with that id (never one found by host, name, or address), and the discovered host
 * under that id. The hub's own cursor row wins while the hub renders.
 */
import { describe, it, expect, vi } from 'vitest'

vi.mock('./network-store.svelte', () => ({ getNetworkHosts: () => [] }))
vi.mock('#lib/tauri-commands/index.js', () => ({ listSavedServers: vi.fn(() => Promise.resolve([])) }))

import type { FileEntry, NetworkHost } from '../types'
import type { SavedServer } from '#lib/tauri-commands/index.js'
import type { ServerMeta } from '#lib/ipc/bindings.js'
import type { HubRow } from './servers-hub-rows'
import { serverRowOf, serverRowUnderCursor } from './server-row-of-entry'

const serverMeta: ServerMeta = {
  rowId: 'manual-naspolya',
  protocol: 'smb',
  address: '192.168.1.111',
  status: 'connected',
  account: { kind: 'user', username: 'test_user_1' },
  saved: true,
  place: null,
  pinned: false,
}

const entry = {
  name: 'Naspolya',
  path: 'smb://naspolya',
  isDirectory: true,
  isSymlink: false,
  iconId: 'network:server-smb',
  modifiedAt: 1_790_000_000,
  serverMeta,
} as FileEntry

const savedByThisId = { id: 'manual-naspolya', displayName: 'Naspolya', places: [] } as unknown as SavedServer
const savedAtSameAddress = { id: 'manual-other', address: '192.168.1.111', places: [] } as unknown as SavedServer
const hostById = { id: 'manual-naspolya', name: 'naspolya', port: 445, source: 'manual' } as NetworkHost
const hostByName = { id: 'bonjour-naspolya', name: 'Naspolya', port: 445, source: 'discovered' } as NetworkHost

describe('serverRowOf', () => {
  it('reads the saved entry and the host by the row’s own id only', () => {
    const row = serverRowOf(entry, [savedAtSameAddress, savedByThisId], [hostByName, hostById])
    expect(row).toMatchObject({
      id: 'manual-naspolya',
      kind: 'server',
      name: 'Naspolya',
      protocol: 'smb',
      status: 'connected',
      account: { kind: 'user', username: 'test_user_1' },
      volumeId: null,
      lastConnectedAt: new Date(1_790_000_000 * 1000).toISOString(),
    })
    expect(row?.saved).toBe(savedByThisId)
    expect(row?.host).toBe(hostById)
  })

  it('has nothing saved behind a host Cmdr only found, even when a saved entry shares its id', () => {
    const found = { ...entry, serverMeta: { ...serverMeta, saved: false } }
    expect(serverRowOf(found, [savedByThisId], [])?.saved).toBeNull()
  })

  it('is no row for an entry with no server behind it', () => {
    expect(serverRowOf({ ...entry, serverMeta: undefined }, [], [])).toBeNull()
  })
})

describe('serverRowUnderCursor', () => {
  it('takes the hub’s own cursor row while the hub renders, reading no listing', async () => {
    const hubRow = { id: 'hub-row' } as HubRow
    const refreshCursorEntry = vi.fn(() => Promise.resolve(entry))
    const row = await serverRowUnderCursor({
      getNetworkCursorEntry: () => ({ kind: 'server', row: hubRow }),
      refreshCursorEntry,
    })
    expect(row).toBe(hubRow)
    expect(refreshCursorEntry).not.toHaveBeenCalled()
  })

  it('reads the listing’s cursor entry otherwise', async () => {
    const row = await serverRowUnderCursor({
      getNetworkCursorEntry: () => null,
      refreshCursorEntry: () => Promise.resolve(entry),
    })
    expect(row?.id).toBe('manual-naspolya')
  })
})
