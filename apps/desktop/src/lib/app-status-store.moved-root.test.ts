/**
 * What loading a stored tab does when its volume came back at another root (plan D8): a drive
 * on another letter, a disk at another mount point. The path follows the DISK, rebased before
 * the existence walk, so the walk never probes the old root, which may hold another disk now.
 * And while the startup sweep hasn't registered the volume, the tab keeps its stored path and
 * id untouched. Same fake `@tauri-apps/plugin-store` rig as `app-status-store.smb-tabs.test.ts`.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { loadPaneTabs } from './app-status-store'
import type { PersistedTab } from './file-explorer/tabs/tab-types'
import type { RestoredTabPath } from '#lib/ipc/bindings.js'

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

function storedTab(overrides: Partial<PersistedTab>): PersistedTab {
  return {
    id: 'tab-1',
    path: '/Volumes/Backup/photos/2026',
    volumeId: 'vol-backup-1234567890abcdef',
    sortBy: 'name',
    sortOrder: 'ascending',
    viewMode: 'full',
    pinned: false,
    ...overrides,
  }
}

const keep = (): Promise<RestoredTabPath> => Promise.resolve({ kind: 'keep' })

beforeEach(() => {
  disk.clear()
})

describe('a stored tab whose volume moved', () => {
  it('lands on the same folder under the volume’s new root, probed there', async () => {
    disk.set('leftTabs', { tabs: [storedTab({})], activeTabId: 'tab-1' })
    const probed: string[] = []
    const exists = (p: string) => {
      probed.push(p)
      return Promise.resolve(p.startsWith('/Volumes/Backup 1'))
    }
    const livePath = vi.fn(({ volumeId, path }: { volumeId: string; path: string }): Promise<RestoredTabPath> =>
      Promise.resolve(
        volumeId === 'vol-backup-1234567890abcdef'
          ? { kind: 'moved', path: path.replace('/Volumes/Backup', '/Volumes/Backup 1') }
          : { kind: 'keep' },
      ),
    )

    const paneTabs = await loadPaneTabs('left', exists, livePath)

    expect(livePath).toHaveBeenCalledWith({
      volumeId: 'vol-backup-1234567890abcdef',
      path: '/Volumes/Backup/photos/2026',
    })
    expect(paneTabs.tabs[0].path).toBe('/Volumes/Backup 1/photos/2026')
    expect(probed.some((p) => p.startsWith('/Volumes/Backup/'))).toBe(false)
    expect(paneTabs.registryPendingTabIds).toEqual([])
  })

  it('keeps the path when the volume’s root is where it was', async () => {
    disk.set('leftTabs', { tabs: [storedTab({ path: '/Users/test_user_1', volumeId: 'root' })], activeTabId: 'tab-1' })

    const paneTabs = await loadPaneTabs('left', () => Promise.resolve(true), keep)

    expect(paneTabs.tabs[0]).toMatchObject({ path: '/Users/test_user_1', volumeId: 'root' })
  })
})

describe('a stored tab whose volume the startup sweep hasn’t registered yet', () => {
  it('keeps its stored path and id, never probes the old root, and says it’s pending', async () => {
    disk.set('leftTabs', {
      tabs: [storedTab({ path: 'E:/photos', volumeId: 'vol-stick-a' }), storedTab({ id: 'tab-2', path: '/Users/x' })],
      activeTabId: 'tab-1',
    })
    const probed: string[] = []
    const exists = (p: string) => {
      probed.push(p)
      return Promise.resolve(true)
    }
    const livePath = ({ volumeId }: { volumeId: string }): Promise<RestoredTabPath> =>
      Promise.resolve(volumeId === 'vol-stick-a' ? { kind: 'notReady' } : { kind: 'keep' })

    const paneTabs = await loadPaneTabs('left', exists, livePath)

    expect(paneTabs.tabs[0]).toMatchObject({ path: 'E:/photos', volumeId: 'vol-stick-a' })
    expect(probed).not.toContain('E:/photos')
    expect(paneTabs.registryPendingTabIds).toEqual(['tab-1'])
  })
})
