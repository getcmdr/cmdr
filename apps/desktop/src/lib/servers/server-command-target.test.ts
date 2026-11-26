/**
 * Which server the palette's server commands act on.
 *
 * The Servers list IS a pane, so "the focused pane's volume" answers the synthetic Servers row
 * rather than the server the cursor is on. The cursor row wins wherever there is
 * one; the pane's own volume is the fallback for a pane standing inside a server.
 */

import { describe, it, expect } from 'vitest'
import { serverCommandTarget, type ServerCommandSources } from './server-command-target'
import type { ServerRow } from '#lib/file-explorer/network/server-row-of-entry.js'
import type { NetworkHost, VolumeInfo } from '#lib/file-explorer/types.js'

function row(overrides: Partial<ServerRow> = {}): ServerRow {
  return {
    id: 'sftp-testnas.local-22-test_user_1',
    path: 'sftp://test_user_1@testnas.local:22',
    account: null,
    name: 'Testnas',
    protocol: 'sftp',
    address: 'testnas.local:22',
    status: 'saved',
    volumeId: 'sftp-testnas.local-22-test_user_1',
    pinned: true,
    saved: null,
    host: null,
    ...overrides,
  }
}

function volume(overrides: Partial<VolumeInfo> = {}): VolumeInfo {
  return {
    id: 'webdav-testnas.local-5006-test_user_1',
    name: 'Nextcloud',
    path: 'webdav://test_user_1@testnas.local:5006',
    category: 'network',
    isEjectable: false,
    fsType: 'webdav',
    connectionState: 'direct',
    ...overrides,
  }
}

const nas: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'testnas.local',
  port: 445,
  source: 'discovered',
}

/** The three readings, none of them pointing anywhere unless a test says so. */
function sources(overrides: Partial<ServerCommandSources> = {}): ServerCommandSources {
  return { cursorRow: null, shareListHost: null, paneVolume: null, ...overrides }
}

describe('serverCommandTarget', () => {
  it('takes the Servers row under the cursor when the focused pane is the Servers list', () => {
    const target = serverCommandTarget(sources({ cursorRow: row() }))
    expect(target).toEqual({
      kind: 'place',
      volumeId: 'sftp-testnas.local-22-test_user_1',
      name: 'Testnas',
      pinned: true,
    })
  })

  it('falls back to the focused pane’s own volume when it is a server place', () => {
    const target = serverCommandTarget(sources({ paneVolume: volume() }))
    expect(target).toEqual({
      kind: 'place',
      volumeId: 'webdav-testnas.local-5006-test_user_1',
      name: 'Nextcloud',
      pinned: null,
    })
  })

  it('prefers the cursor row over the pane volume, because the cursor is what the user is looking at', () => {
    const target = serverCommandTarget(sources({ cursorRow: row(), paneVolume: volume() }))
    expect(target?.kind === 'place' && target.volumeId).toBe('sftp-testnas.local-22-test_user_1')
  })

  it('takes an SMB host row as its SERVER, by its host: it has no volume of its own', () => {
    const hostRow = row({ protocol: 'smb', volumeId: null, host: nas })
    expect(serverCommandTarget(sources({ cursorRow: hostRow }))).toEqual({
      kind: 'smb-server',
      host: nas,
      name: 'Testnas',
      row: hostRow,
    })
  })

  it('takes the server whose share list the pane shows, when the pane isn’t the hub', () => {
    expect(serverCommandTarget(sources({ shareListHost: nas }))).toEqual({
      kind: 'smb-server',
      host: nas,
      name: 'Testnas',
      row: null,
    })
  })

  it('answers nothing for an S3 account row, ❌ never the pane’s volume instead', () => {
    const account = row({ protocol: 's3', volumeId: null })
    expect(serverCommandTarget(sources({ cursorRow: account, paneVolume: volume() }))).toBeNull()
  })

  it('answers nothing for the synthetic Servers volume, which is not a server', () => {
    const hub = volume({ id: 'network', name: 'Servers', path: 'smb://', fsType: 'smbfs', connectionState: null })
    expect(serverCommandTarget(sources({ paneVolume: hub }))).toBeNull()
  })

  it('answers nothing for a mounted SMB share: its session is an OS mount the family doesn’t speak', () => {
    const share = volume({ id: 'smb-share-1', name: 'Public', path: '/Volumes/Public', fsType: 'smbfs' })
    expect(serverCommandTarget(sources({ paneVolume: share }))).toBeNull()
  })

  it('answers nothing for a local disk', () => {
    const local = volume({ id: 'root', name: 'Macintosh HD', path: '/', category: 'main_volume', fsType: 'apfs' })
    expect(serverCommandTarget(sources({ paneVolume: local }))).toBeNull()
  })
})
