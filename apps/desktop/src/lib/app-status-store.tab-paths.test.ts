/**
 * The path forms a stored tab keeps.
 *
 * A tab's `path` is saved as the pane held it and handed back as is when its folder
 * still exists: every spelling below, a trailing `/` included, round-trips byte for
 * byte. The app-path type normalizes at use time, so no stored tab changes (#1225).
 * Same fake `@tauri-apps/plugin-store` rig as `app-status-store.snapshot-paths.test.ts`.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { loadPaneTabs, savePaneTabs } from './app-status-store'
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

const alwaysExists = () => Promise.resolve(true)

const spellings: [string, string][] = [
  ['/Users/test_user_1/a/', 'root'],
  ['/Users/test_user_1/a b/c-d', 'root'],
  ['/srv/a\\b/c:d', 'root'],
  ['/Users/test_user_1/café', 'root'],
  ['/Volumes/testshare/docs/', 'smb-testnas-445-testshare'],
  ['sftp://test_user_1@testnas.local:22/srv/data', 'sftp-testnas-22'],
]

function tab(path: string, volumeId: string, index: number): PersistedTab {
  return {
    id: `tab-${String(index)}`,
    path,
    volumeId,
    sortBy: 'name',
    sortOrder: 'ascending',
    viewMode: 'full',
    pinned: false,
  }
}

beforeEach(() => {
  disk.clear()
})

describe('a stored tab path', () => {
  it('comes back byte for byte when its folder exists', async () => {
    const tabs = spellings.map(([path, volumeId], i) => tab(path, volumeId, i))
    await savePaneTabs('left', { tabs, activeTabId: 'tab-0' })

    const loaded = await loadPaneTabs('left', alwaysExists)

    expect(loaded.tabs.map((t) => t.path)).toEqual(spellings.map(([path]) => path))
    expect(loaded.activeTabId).toBe('tab-0')
  })
})
