/**
 * What loading a stored tab does with a server's share list (`network` at `smb://<server>`).
 *
 * ❗ It comes back as the Servers list, with the share-list path kept as where its
 * cursor lands: listing the server at launch is a dial and a possible Keychain read
 * nobody asked for (#324). The normalization runs FIRST, so `live_volume_path` and the
 * existence walk are never asked about a server. Same fake store rig as
 * `app-status-store.smb-tabs.test.ts`.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { loadAppStatus, loadPaneTabs, type LivePathFn } from './app-status-store'
import type { PersistedTab } from './file-explorer/tabs/tab-types'

const disk = vi.hoisted(() => new Map<string, unknown>())

vi.mock('@tauri-apps/plugin-store', () => ({
  load: vi.fn(() =>
    Promise.resolve({
      get: (key: string) => Promise.resolve(disk.get(key)),
      set: (key: string, value: unknown) => {
        disk.set(key, value)
        return Promise.resolve()
      },
      delete: (key: string) => Promise.resolve(disk.delete(key)),
      has: (key: string) => Promise.resolve(disk.has(key)),
      keys: () => Promise.resolve([...disk.keys()]),
      save: () => Promise.resolve(),
    }),
  ),
}))

vi.mock('./settings/store-path', () => ({
  resolveStorePath: (name: string) => Promise.resolve(name),
}))

const pathExists = vi.fn((p: string) => Promise.resolve(p === '/' || p === '~'))
const livePath = vi.fn<LivePathFn>(() => Promise.resolve({ kind: 'keep' }))

function storedTab(overrides: Partial<PersistedTab>): PersistedTab {
  return {
    id: 'tab-1',
    path: 'smb://testnas',
    volumeId: 'network',
    sortBy: 'name',
    sortOrder: 'ascending',
    viewMode: 'full',
    pinned: false,
    ...overrides,
  }
}

beforeEach(() => {
  disk.clear()
  pathExists.mockClear()
  livePath.mockClear()
})

describe("a stored tab on a server's share list", () => {
  it('comes back as the Servers list, landing on that server, and asks nothing about the server', async () => {
    disk.set('leftTabs', { tabs: [storedTab({})], activeTabId: 'tab-1' })

    const paneTabs = await loadPaneTabs('left', pathExists, livePath)

    expect(paneTabs.tabs[0]).toMatchObject({ path: 'smb://', volumeId: 'network' })
    expect(paneTabs.landOnServerByTabId).toEqual({ 'tab-1': 'smb://testnas' })
    expect(livePath).not.toHaveBeenCalled()
    expect(pathExists).not.toHaveBeenCalled()
  })

  it('carries a landing for a background tab too, and none for a tab on the Servers list itself', async () => {
    disk.set('leftTabs', {
      tabs: [storedTab({ id: 'servers', path: 'smb://' }), storedTab({ id: 'nas', path: 'smb://testnas:4450' })],
      activeTabId: 'servers',
    })

    const paneTabs = await loadPaneTabs('left', pathExists, livePath)

    expect(paneTabs.tabs.map((t) => t.path)).toEqual(['smb://', 'smb://'])
    expect(paneTabs.landOnServerByTabId).toEqual({ nas: 'smb://testnas:4450' })
  })

  it('comes back as the Servers list through the scalar-key migration too', async () => {
    disk.set('leftPath', 'smb://testnas')
    disk.set('leftVolumeId', 'network')

    const paneTabs = await loadPaneTabs('left', pathExists, livePath)

    expect(paneTabs.tabs[0]).toMatchObject({ path: 'smb://', volumeId: 'network' })
    expect(paneTabs.landOnServerByTabId).toEqual({ [paneTabs.tabs[0].id]: 'smb://testnas' })
  })

  it('comes back as the Servers list on the scalar pane keys as well', async () => {
    disk.set('leftPath', 'smb://testnas')
    disk.set('leftVolumeId', 'network')

    const status = await loadAppStatus(pathExists)

    expect(status).toMatchObject({ leftPath: 'smb://', leftVolumeId: 'network' })
  })

  it('leaves a direct SMB volume`s own `smb://` path alone: only the network volume lists servers', async () => {
    disk.set('leftTabs', {
      tabs: [storedTab({ volumeId: 'smb-testnas-public', path: 'smb://testnas' })],
      activeTabId: 'tab-1',
    })

    const paneTabs = await loadPaneTabs('left', pathExists, livePath)

    expect(paneTabs.tabs[0]).toMatchObject({ path: 'smb://testnas', volumeId: 'smb-testnas-public' })
    expect(paneTabs.landOnServerByTabId).toEqual({})
  })
})
