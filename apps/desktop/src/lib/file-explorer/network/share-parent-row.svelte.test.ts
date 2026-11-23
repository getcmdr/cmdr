/**
 * The frontend's cache of where `..` leads out of each SMB share (#1238): what it reads
 * from Rust, what a share shows per (default, override), and that every write goes
 * through Rust before the cache moves. Each test uses its own volume id, since the
 * cache is module state.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { VolumeInfo } from '../types'

const { getSmbShareParent, setSmbShareParentRow, settings } = vi.hoisted(() => ({
  getSmbShareParent: vi.fn(),
  setSmbShareParentRow: vi.fn(),
  settings: { showShareParentRow: true },
}))
vi.mock('#lib/tauri-commands/index.js', () => ({ getSmbShareParent, setSmbShareParentRow }))
vi.mock('#lib/settings/reactive-settings.svelte.js', () => ({
  getShowShareParentRow: () => settings.showShareParentRow,
}))
vi.mock('#lib/settings/index.js', () => ({
  getSetting: () => settings.showShareParentRow,
  setSetting: (_id: string, value: boolean) => {
    settings.showShareParentRow = value
  },
}))

import {
  isSmbShareVolume,
  loadShareParent,
  setShareParentRow,
  shareParentOf,
  showsParentRow,
  toggleShareParentRow,
  turnOffShareParentRowForAll,
} from './share-parent-row.svelte'

const host = { id: 'testnas', name: 'Testnas', hostname: 'testnas.local', port: 445 }

function parent(showParentRow: boolean | null) {
  return { host, serverLabel: 'Testnas', shareName: 'media', showParentRow }
}

beforeEach(() => {
  vi.clearAllMocks()
  settings.showShareParentRow = true
})

describe('isSmbShareVolume', () => {
  it('knows an SMB share from a local drive and from nothing at all', () => {
    const share = { id: 'smb-testnas-media', path: '/Volumes/media', category: 'network', fsType: 'smbfs' }
    const local = { id: 'root', path: '/', category: 'main_volume', fsType: 'apfs' }
    expect(isSmbShareVolume(share as unknown as VolumeInfo)).toBe(true)
    expect(isSmbShareVolume(local as unknown as VolumeInfo)).toBe(false)
    expect(isSmbShareVolume(null)).toBe(false)
    expect(isSmbShareVolume(undefined)).toBe(false)
  })
})

describe('loadShareParent', () => {
  it('caches Rust’s answer for the share', async () => {
    getSmbShareParent.mockResolvedValue(parent(null))
    expect(shareParentOf('v-load')).toBeUndefined()

    await expect(loadShareParent('v-load')).resolves.toMatchObject({ shareName: 'media' })
    expect(shareParentOf('v-load')).toMatchObject({ serverLabel: 'Testnas' })
  })

  it('caches “no share behind it” too', async () => {
    getSmbShareParent.mockResolvedValue(null)
    await loadShareParent('v-none')
    expect(shareParentOf('v-none')).toBeNull()
    expect(showsParentRow('v-none')).toBeUndefined()
  })

  it('keeps what it knew when Rust breaks down, and never throws', async () => {
    getSmbShareParent.mockResolvedValueOnce(parent(false))
    await loadShareParent('v-flaky')
    getSmbShareParent.mockRejectedValueOnce(new Error('ipc down'))

    await expect(loadShareParent('v-flaky')).resolves.toMatchObject({ showParentRow: false })
    await expect(loadShareParent('v-never-asked-broken')).resolves.toBeNull()
  })
})

describe('showsParentRow', () => {
  it('follows the default without an override, and the override over it', async () => {
    getSmbShareParent.mockResolvedValueOnce(parent(null))
    await loadShareParent('v-default')
    getSmbShareParent.mockResolvedValueOnce(parent(false))
    await loadShareParent('v-off')

    expect(showsParentRow('v-default')).toBe(true)
    expect(showsParentRow('v-off')).toBe(false)
    settings.showShareParentRow = false
    expect(showsParentRow('v-default')).toBe(false)
  })

  it('says nothing before Rust has answered', () => {
    expect(showsParentRow('v-unknown')).toBeUndefined()
  })
})

describe('setShareParentRow', () => {
  it('saves against the current default, and clears the override when the choice matches it', async () => {
    getSmbShareParent.mockResolvedValue(parent(null))
    await loadShareParent('v-set')
    setSmbShareParentRow.mockResolvedValue(true)

    await expect(setShareParentRow('v-set', false)).resolves.toBe(true)
    expect(setSmbShareParentRow).toHaveBeenLastCalledWith('v-set', false, true)
    expect(shareParentOf('v-set')?.showParentRow).toBe(false)

    await setShareParentRow('v-set', true)
    expect(shareParentOf('v-set')?.showParentRow).toBeNull()
  })

  it('leaves the cache alone when Rust saved nothing', async () => {
    getSmbShareParent.mockResolvedValue(parent(null))
    await loadShareParent('v-gone')
    setSmbShareParentRow.mockResolvedValue(false)

    await expect(setShareParentRow('v-gone', false)).resolves.toBe(false)
    expect(shareParentOf('v-gone')?.showParentRow).toBeNull()
  })

  it('answers false when Rust breaks down', async () => {
    setSmbShareParentRow.mockRejectedValue(new Error('ipc down'))
    await expect(setShareParentRow('v-broken', false)).resolves.toBe(false)
  })
})

describe('toggleShareParentRow', () => {
  it('asks Rust first when nothing is cached, then flips what the share shows', async () => {
    getSmbShareParent.mockResolvedValue(parent(null))
    setSmbShareParentRow.mockResolvedValue(true)

    await toggleShareParentRow('v-toggle')

    expect(getSmbShareParent).toHaveBeenCalledWith('v-toggle')
    expect(setSmbShareParentRow).toHaveBeenCalledWith('v-toggle', false, true)
    expect(showsParentRow('v-toggle')).toBe(false)
  })

  it('does nothing on a volume with no share behind it', async () => {
    getSmbShareParent.mockResolvedValue(null)
    await toggleShareParentRow('v-toggle-none')
    expect(setSmbShareParentRow).not.toHaveBeenCalled()
  })
})

describe('turnOffShareParentRowForAll', () => {
  it('turns the default off and keeps a share someone turned on', async () => {
    getSmbShareParent.mockResolvedValueOnce(parent(true))
    await loadShareParent('v-kept-on')
    getSmbShareParent.mockResolvedValueOnce(parent(null))
    await loadShareParent('v-follows')

    turnOffShareParentRowForAll()

    expect(settings.showShareParentRow).toBe(false)
    expect(showsParentRow('v-kept-on')).toBe(true)
    expect(showsParentRow('v-follows')).toBe(false)
  })
})
