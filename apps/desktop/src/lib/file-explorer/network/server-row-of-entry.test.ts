/**
 * A Servers list row becomes the row every server action takes BY ITS OWN ID (the saved
 * entry with that id, never one found by host, name, or address), and an SMB server's host
 * is the one its share list resolves to, by the row's path: discovery's host under that
 * path, else the host the path itself names. ❌ Never a host found by the row id or parsed
 * out of the Details text.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

import type { FileEntry, NetworkHost } from '../types'

let hosts: NetworkHost[] = []
vi.mock('./network-store.svelte', () => ({
  getNetworkHosts: () => hosts,
  setListedAccount: vi.fn(),
  getResolvedShareListHost: () => undefined,
  noteResolvedShareListHost: vi.fn(),
}))
vi.mock('./share-parent-row.svelte', () => ({ shareParentServers: () => [] }))
vi.mock('#lib/tauri-commands/index.js', () => ({ listSavedServers: vi.fn(() => Promise.resolve([])) }))

import type { SavedServer } from '#lib/tauri-commands/index.js'
import type { ServerMeta } from '#lib/ipc/bindings.js'
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

/** A server saved by IP and shown under its Bonjour twin's name: its row opens the share list at the saved address. */
const entry = {
  name: 'Naspolya',
  path: 'smb://192.168.1.111',
  isDirectory: true,
  isSymlink: false,
  iconId: 'network:server-smb',
  modifiedAt: 1_790_000_000,
  serverMeta,
} as FileEntry

const savedByThisId = { id: 'manual-naspolya', displayName: 'Naspolya', places: [] } as unknown as SavedServer
const savedAtSameAddress = { id: 'manual-other', address: '192.168.1.111', places: [] } as unknown as SavedServer
/** The Bonjour twin discovery holds: same machine, its OWN share list (`smb://naspolya`). */
const bonjourTwin: NetworkHost = {
  id: 'bonjour-naspolya',
  name: 'Naspolya',
  hostname: 'naspolya.local',
  ipAddress: '192.168.1.111',
  port: 445,
  source: 'discovered',
}
/** A host discovery holds under the row's id, at another address: never the row's host by that alone. */
const hostById: NetworkHost = {
  id: 'manual-naspolya',
  name: 'elsewhere',
  hostname: '10.0.0.9',
  port: 445,
  source: 'manual',
}
/** The manual host discovery holds for the saved address: its share list IS the row's path. */
const manualAtSavedAddress: NetworkHost = {
  id: 'manual-192.168.1.111',
  name: '192.168.1.111',
  hostname: '192.168.1.111',
  port: 445,
  source: 'manual',
}

beforeEach(() => {
  hosts = []
})

describe('serverRowOf', () => {
  it('reads the saved entry by the row’s own id only', () => {
    const row = serverRowOf(entry, [savedAtSameAddress, savedByThisId])
    expect(row).toMatchObject({
      id: 'manual-naspolya',
      path: 'smb://192.168.1.111',
      name: 'Naspolya',
      protocol: 'smb',
      status: 'connected',
      account: { kind: 'user', username: 'test_user_1' },
      volumeId: null,
    })
    expect(row?.saved).toBe(savedByThisId)
  })

  it('takes the host its share list resolves to, by the row’s path, ❌ never the host under its id or its twin', () => {
    hosts = [bonjourTwin, hostById, manualAtSavedAddress]
    expect(serverRowOf(entry, [])?.host).toBe(manualAtSavedAddress)
  })

  it('stands for the host its path names when discovery holds none at that path', () => {
    hosts = [bonjourTwin, hostById]
    expect(serverRowOf(entry, [])?.host).toEqual({
      id: 'manual-naspolya',
      name: 'Naspolya',
      hostname: '192.168.1.111',
      port: 445,
      source: 'manual',
    })
  })

  it('keeps the port off 445 the path carries', () => {
    const onPort = { ...entry, path: 'smb://localhost:11482' }
    expect(serverRowOf(onPort, [])?.host).toMatchObject({ hostname: 'localhost', port: 11482 })
  })

  it('has no host for a server that isn’t SMB', () => {
    const sftp = { ...entry, path: 'sftp://test_user_1@testnas:22', serverMeta: { ...serverMeta, protocol: 'sftp' } }
    expect(serverRowOf(sftp as FileEntry, [])?.host).toBeNull()
  })

  it('has nothing saved behind a host Cmdr only found, even when a saved entry shares its id', () => {
    const found = { ...entry, serverMeta: { ...serverMeta, saved: false } }
    expect(serverRowOf(found, [savedByThisId])?.saved).toBeNull()
  })

  it('is no row for an entry with no server behind it', () => {
    expect(serverRowOf({ ...entry, serverMeta: undefined }, [])).toBeNull()
  })
})

describe('serverRowUnderCursor', () => {
  it('reads the listing’s cursor entry', async () => {
    const row = await serverRowUnderCursor({ refreshCursorEntry: () => Promise.resolve(entry) })
    expect(row?.id).toBe('manual-naspolya')
  })

  it('is nothing off the Servers list or on its trailing row', async () => {
    expect(await serverRowUnderCursor({ refreshCursorEntry: () => Promise.resolve(null) })).toBeNull()
    expect(await serverRowUnderCursor(undefined)).toBeNull()
  })
})
