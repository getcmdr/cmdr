/**
 * Enter on a row the `network` volume lists that leads OFF it: a one-place server lands the
 * pane on its place (the pane dials there, `place-connect`), an S3 account saves its root
 * and lands there, and a saved share lands on its place when the volume list has one. An
 * SMB server's row is a plain folder on the same volume: the generic open takes it.
 */
import { describe, it, expect, vi } from 'vitest'
import type { FileEntry, VolumeInfo } from '../types'
import type { ServerMeta } from '#lib/ipc/bindings.js'
import { openNetworkRow, type NetworkRowOpenDeps } from './network-row-open'

function serverRow(meta: Partial<ServerMeta>, path: string): FileEntry {
  return {
    name: 'Testhost',
    path,
    isDirectory: true,
    isSymlink: false,
    iconId: 'network:server',
    serverMeta: {
      rowId: 'row-1',
      protocol: 'sftp',
      address: 'testhost:22',
      status: 'saved',
      account: null,
      saved: true,
      place: null,
      pinned: false,
      ...meta,
    },
  } as FileEntry
}

function shareRow(saved: { volumeId: string } | null): FileEntry {
  return {
    name: 'archive',
    path: 'smb://testnas/archive',
    isDirectory: true,
    isSymlink: false,
    iconId: 'network:share',
    shareMeta: {
      description: null,
      saved: saved ? { volumeId: saved.volumeId, status: 'saved', account: null, pinned: false } : null,
    },
  } as FileEntry
}

function deps(volumes: Partial<VolumeInfo>[] = []) {
  return {
    onVolumeChange: vi.fn<NetworkRowOpenDeps['onVolumeChange']>(),
    getVolumes: () => volumes as VolumeInfo[],
    saveS3AccountRoot: vi.fn<NetworkRowOpenDeps['saveS3AccountRoot']>(() => Promise.resolve(true)),
    explainNoAccountRoot: vi.fn<NetworkRowOpenDeps['explainNoAccountRoot']>(),
  } satisfies NetworkRowOpenDeps
}

describe('openNetworkRow', () => {
  it('lands a one-place server on its saved place, at its start folder', () => {
    const d = deps([
      {
        id: 'sftp-1',
        path: 'sftp://test_user_1@testhost:22',
        connectionState: 'saved',
        landingPath: 'sftp://test_user_1@testhost:22/srv',
      },
    ])
    const took = openNetworkRow(serverRow({ place: 'sftp-1' }, 'sftp://test_user_1@testhost:22'), d)
    expect(took).toBe(true)
    expect(d.onVolumeChange).toHaveBeenCalledWith({
      volumeId: 'sftp-1',
      volumePath: 'sftp://test_user_1@testhost:22',
      targetPath: 'sftp://test_user_1@testhost:22/srv',
    })
  })

  it('lands a one-place server the volume list doesn’t hold on its row’s path', () => {
    const d = deps()
    openNetworkRow(serverRow({ place: 'webdav-1', protocol: 'webdav' }, 'webdav://testhost/dav'), d)
    expect(d.onVolumeChange).toHaveBeenCalledWith({
      volumeId: 'webdav-1',
      volumePath: 'webdav://testhost/dav',
      targetPath: 'webdav://testhost/dav',
    })
  })

  it('saves an S3 account’s root, then lands on it', async () => {
    const d = deps()
    const took = openNetworkRow(serverRow({ protocol: 's3', rowId: 's3-acct' }, 's3://KEY@s3.example:443/'), d)
    expect(took).toBe(true)
    await vi.waitFor(() => {
      expect(d.onVolumeChange).toHaveBeenCalledWith({
        volumeId: 's3-acct',
        volumePath: 's3://KEY@s3.example:443/',
        targetPath: 's3://KEY@s3.example:443/',
      })
    })
    expect(d.saveS3AccountRoot).toHaveBeenCalledWith('s3-acct')
  })

  it('says why when an S3 account’s root can’t be saved, and goes nowhere', async () => {
    const d = deps()
    d.saveS3AccountRoot.mockResolvedValueOnce(false)
    openNetworkRow(serverRow({ protocol: 's3', rowId: 's3-acct' }, 's3://KEY@s3.example:443/'), d)
    await vi.waitFor(() => {
      expect(d.explainNoAccountRoot).toHaveBeenCalledWith('Testhost')
    })
    expect(d.onVolumeChange).not.toHaveBeenCalled()
  })

  it('leaves an SMB server’s row to the generic folder open (it’s its share list)', () => {
    const d = deps()
    expect(openNetworkRow(serverRow({ protocol: 'smb', place: null }, 'smb://testnas'), d)).toBe(false)
    expect(d.onVolumeChange).not.toHaveBeenCalled()
  })

  it('lands a saved share on its place when the volume list holds it', () => {
    const d = deps([{ id: 'smb-share-1', path: '/Volumes/archive', connectionState: 'os_mount' }])
    expect(openNetworkRow(shareRow({ volumeId: 'smb-share-1' }), d)).toBe(true)
    expect(d.onVolumeChange).toHaveBeenCalledWith({
      volumeId: 'smb-share-1',
      volumePath: '/Volumes/archive',
      targetPath: '/Volumes/archive',
    })
  })

  it('leaves a share with no place in the volume list to the share list’s mount', () => {
    const d = deps()
    expect(openNetworkRow(shareRow({ volumeId: 'smb-share-1' }), d)).toBe(false)
    expect(openNetworkRow(shareRow(null), d)).toBe(false)
    expect(d.onVolumeChange).not.toHaveBeenCalled()
  })
})
