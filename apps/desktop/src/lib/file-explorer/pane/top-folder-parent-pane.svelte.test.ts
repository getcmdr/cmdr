/**
 * One pane's `..` out of a server's top folder, every kind through one seam: whether it shows
 * (each switch), where it leads, and that a listing of another folder says nothing about it.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushSync } from 'svelte'
import type { NetworkHost, VolumeInfo } from '../types'
import type { TopFolderParent } from '#lib/tauri-commands/index.js'

const { getSmbShareParent, settings } = vi.hoisted(() => ({
  getSmbShareParent: vi.fn(),
  settings: { share: true, server: true },
}))
vi.mock('#lib/tauri-commands/index.js', () => ({ getSmbShareParent, setSmbShareParentRow: vi.fn() }))
vi.mock('#lib/settings/reactive-settings.svelte.js', () => ({
  getShowShareParentRow: () => settings.share,
  getShowServerParentRow: () => settings.server,
}))
vi.mock('#lib/settings/index.js', () => ({ getSetting: () => settings.share, setSetting: vi.fn() }))

import { createTopFolderParentPane, type TopFolderParentPane } from './top-folder-parent-pane.svelte'

const host: NetworkHost = { id: 'testnas', name: 'Testnas', hostname: 'testnas.local', port: 445 }
const share = {
  id: 'smb-testnas-media',
  name: 'media on Testnas',
  path: '/Volumes/media',
  category: 'network',
  fsType: 'smbfs',
} as unknown as VolumeInfo
const sftp = {
  id: 'sftp-testnas-22',
  name: 'Testnas',
  path: 'sftp://test_user_1@testnas.local:22/srv',
  category: 'network',
} as unknown as VolumeInfo

const toServersList: TopFolderParent = {
  volumeId: 'network',
  path: 'smb://',
  selectPath: 'sftp://test_user_1@testnas.local:22/srv',
  switch: 'server',
}
const toShareList: TopFolderParent = { volumeId: 'network', path: 'smb://testnas', selectPath: null, switch: 'share' }

/** What the live volume list calls each volume a `..` can lead to. */
const volumeNames: Record<string, string> = { 's3-account-root': 'Test storage' }

let dispose: (() => void) | undefined
afterEach(() => {
  dispose?.()
  dispose = undefined
})

beforeEach(() => {
  vi.clearAllMocks()
  settings.share = true
  settings.server = true
})

async function create(volume: VolumeInfo, path: string) {
  getSmbShareParent.mockResolvedValue({ host, serverLabel: 'Testnas', shareName: 'media', showParentRow: null })
  const openServer = vi.fn()
  const goTo = vi.fn()
  const current = { path }
  let pane!: TopFolderParentPane
  dispose = $effect.root(() => {
    pane = createTopFolderParentPane({
      getVolume: () => volume,
      getCurrentPath: () => current.path,
      getVolumeName: (volumeId) => volumeNames[volumeId] ?? null,
      openServer,
      goTo,
    })
  })
  flushSync()
  await Promise.resolve()
  await Promise.resolve()
  flushSync()
  return { pane, openServer, goTo, current }
}

describe('createTopFolderParentPane', () => {
  it('leads a place’s top folder to its Servers row, as Rust said', async () => {
    const { pane, goTo } = await create(sftp, sftp.path)
    pane.note({ path: sftp.path, atVolumeTop: true, parent: toServersList })
    flushSync()

    expect(pane.atVolumeTop).toBe(true)
    expect(pane.leadsOutHere).toBe(true)
    expect(pane.leadsTo).toBe('smb://')
    expect(pane.leave()).toBe(true)
    expect(goTo).toHaveBeenCalledWith(toServersList)
  })

  it('hides a server’s `..` with its switch off, and Go to parent does nothing', async () => {
    settings.server = false
    const { pane, goTo } = await create(sftp, sftp.path)
    pane.note({ path: sftp.path, atVolumeTop: true, parent: toServersList })
    flushSync()

    expect(pane.leadsOutHere).toBe(false)
    expect(pane.leave()).toBe(false)
    expect(goTo).not.toHaveBeenCalled()
  })

  it('says nothing about a folder another listing was for', async () => {
    const { pane, current } = await create(sftp, sftp.path)
    pane.note({ path: sftp.path, atVolumeTop: true, parent: toServersList })
    current.path = `${sftp.path}/photos`
    flushSync()

    expect(pane.atVolumeTop).toBeUndefined()
    expect(pane.leadsOutHere).toBe(false)
  })

  it('takes a failed top folder’s way up from its error', async () => {
    const { pane, goTo } = await create(sftp, sftp.path)
    pane.note({ path: sftp.path, parent: toServersList })
    flushSync()

    expect(pane.atVolumeTop).toBeUndefined()
    expect(pane.leave()).toBe(true)
    expect(goTo).toHaveBeenCalledWith(toServersList)
  })

  it('always leads a share list up to the Servers list', async () => {
    settings.server = false
    settings.share = false
    const network = { id: 'network', name: 'Network', path: 'smb://', category: 'network' } as unknown as VolumeInfo
    const toServers: TopFolderParent = {
      volumeId: 'network',
      path: 'smb://',
      selectPath: 'smb://testnas',
      switch: 'always',
    }
    const { pane, goTo } = await create(network, 'smb://testnas')
    pane.note({ path: 'smb://testnas', atVolumeTop: false, parent: toServers })
    flushSync()

    expect(pane.leave()).toBe(true)
    expect(goTo).toHaveBeenCalledWith(toServers)
  })

  it('leaves a share through its share list’s door, the cursor landing on the share', async () => {
    const { pane, openServer, goTo } = await create(share, share.path)
    await vi.waitFor(() => {
      expect(getSmbShareParent).toHaveBeenCalledWith(share.id)
    })
    await Promise.resolve()
    pane.note({ path: share.path, atVolumeTop: true, parent: toShareList })
    flushSync()

    expect(pane.leave()).toBe(true)
    expect(openServer).toHaveBeenCalledWith(host, {
      land: { volumeId: share.id, hostId: 'testnas', shareName: 'media', serverLabel: 'Testnas' },
    })
    expect(goTo).not.toHaveBeenCalled()
  })

  describe('rowName: the `..` row names where it leads, the way a share list’s says “Back to Servers”', () => {
    it('says “Back to Servers” on a place’s top folder and on a share list', async () => {
      const { pane } = await create(sftp, sftp.path)
      pane.note({ path: sftp.path, atVolumeTop: true, parent: toServersList })
      flushSync()
      expect(pane.rowName).toBe('Back to Servers')

      pane.note({ path: sftp.path, atVolumeTop: true, parent: { ...toServersList, switch: 'always' } })
      flushSync()
      expect(pane.rowName).toBe('Back to Servers')
    })

    it('names the server whose share list a share’s top folder leads to', async () => {
      const { pane } = await create(share, share.path)
      await vi.waitFor(() => {
        expect(getSmbShareParent).toHaveBeenCalledWith(share.id)
      })
      await Promise.resolve()
      pane.note({ path: share.path, atVolumeTop: true, parent: toShareList })
      flushSync()

      expect(pane.rowName).toBe('Back to Testnas')
    })

    it('names the account whose bucket list a bucket’s top leads to, as the volume list calls it', async () => {
      const toBucketList: TopFolderParent = {
        volumeId: 's3-account-root',
        path: 's3://test_key_1@s3.testnas.local:9000/',
        selectPath: 's3://test_key_1@s3.testnas.local:9000/photos',
        switch: 'server',
      }
      const { pane } = await create(sftp, sftp.path)
      pane.note({ path: sftp.path, atVolumeTop: true, parent: toBucketList })
      flushSync()

      expect(pane.rowName).toBe('Back to Test storage')
    })

    it('says nothing (the plain “Parent folder”) where `..` is the folder above, or its switch hides it', async () => {
      settings.server = false
      const { pane } = await create(sftp, sftp.path)
      expect(pane.rowName).toBeNull()
      pane.note({ path: sftp.path, atVolumeTop: true, parent: toServersList })
      flushSync()
      expect(pane.rowName).toBeNull()
    })
  })

  it('follows the share’s own switch, ❌ not the servers’ one', async () => {
    settings.share = false
    const { pane } = await create(share, share.path)
    await vi.waitFor(() => {
      expect(getSmbShareParent).toHaveBeenCalledWith(share.id)
    })
    await Promise.resolve()
    pane.note({ path: share.path, atVolumeTop: true, parent: toShareList })
    flushSync()

    expect(pane.leadsOutHere).toBe(false)
    expect(pane.leave()).toBe(false)
  })
})
