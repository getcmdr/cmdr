/**
 * The shares a saved SMB server has besides the saved ones (#1404): which show
 * under it, capped, and when the "More shares on …" row stands last.
 */

import { describe, it, expect } from 'vitest'
import { MAX_SHARE_ROWS_PER_SERVER, shareItemMove, shareRowsFor } from './servers-hub-shares'
import type { HubRow } from './servers-hub-rows'
import type { SavedPlace, SavedServer } from '#lib/tauri-commands/index.js'
import type { NetworkHost } from '../types'

function place(name: string): SavedPlace {
  return {
    volumeId: `smb-${name}`,
    name,
    pinned: false,
    connected: false,
    appRoot: `smb://testnas.local/${name}`,
    username: null,
    autoReconnect: null,
  }
}

function server(placeNames: string[], overrides: Partial<HubRow> = {}): HubRow {
  const saved = { id: 'manual-nas', protocol: 'smb', places: placeNames.map(place) } as unknown as SavedServer
  return {
    id: 'manual-nas',
    kind: 'server',
    parentId: null,
    account: null,
    place: null,
    name: 'Testnas',
    protocol: 'smb',
    address: 'testnas.local',
    status: 'saved',
    lastConnectedAt: null,
    volumeId: null,
    pinned: false,
    saved,
    host: null,
    ...overrides,
  }
}

describe('shareRowsFor', () => {
  it('lists the shares not saved yet, by name, without the More row when every share is on screen', () => {
    expect(shareRowsFor(server(['admin']), ['Media', 'admin', 'backup'])).toEqual({
      unsaved: ['backup', 'Media'],
      more: false,
    })
  })

  it('matches a saved share case- and Unicode-form-blind, so it never shows twice', () => {
    const decomposed = 'Café'
    expect(shareRowsFor(server(['ADMIN', 'Café']), ['admin', decomposed, 'Media'])?.unsaved).toEqual(['Media'])
  })

  it('caps the share rows at eight, saved ones included, and puts the rest behind the More row', () => {
    const names = Array.from({ length: 10 }, (_, i) => `share${String(i)}`)
    const rows = shareRowsFor(server(['admin', 'home']), names)
    expect(MAX_SHARE_ROWS_PER_SERVER).toBe(8)
    expect(rows?.unsaved).toHaveLength(6)
    expect(rows?.more).toBe(true)
  })

  it('keeps every saved share even past the cap, and shows only the More row then', () => {
    const saved = Array.from({ length: 9 }, (_, i) => `saved${String(i)}`)
    expect(shareRowsFor(server(saved), ['other'])).toEqual({ unsaved: [], more: true })
  })

  it('shows only the More row while the list is loading, needs a sign-in, or couldn’t be read', () => {
    expect(shareRowsFor(server(['admin']), null)).toEqual({ unsaved: [], more: true })
  })

  it('adds nothing under a server Cmdr only found, a share row, or a non-SMB server', () => {
    expect(shareRowsFor(server([], { saved: null, status: 'found_nearby' }), ['Media'])).toBeNull()
    expect(shareRowsFor(server([], { kind: 'place', parentId: 'manual-nas' }), ['Media'])).toBeNull()
    expect(shareRowsFor(server([], { protocol: 's3' }), ['Media'])).toBeNull()
  })

  it('lists a share the server repeats only once, so no two rows share an id', () => {
    expect(shareRowsFor(server([]), ['Media', 'media'])?.unsaved).toEqual(['Media'])
  })
})

describe('shareItemMove', () => {
  const host: NetworkHost = { id: 'h1', name: 'Testnas', hostname: 'testnas.local', port: 445 }

  it('mounts an unsaved share through its host’s share list, like a saved share no mount went through', () => {
    const nas = server([], { host })
    expect(shareItemMove({ kind: 'unsaved_share', id: 'x', server: nas, name: 'Media' })).toEqual({
      kind: 'share_via_host',
      host,
      share: 'Media',
      label: 'Testnas',
    })
  })

  it('opens the host’s share list from the More row, exactly what Enter on the server row does', () => {
    expect(shareItemMove({ kind: 'more_shares', id: 'y', server: server([], { host }) })).toEqual({
      kind: 'host',
      host,
      label: 'Testnas',
    })
  })

  it('reaches a saved server mDNS isn’t seeing through its saved address', () => {
    const move = shareItemMove({ kind: 'unsaved_share', id: 'x', server: server([]), name: 'Media' })
    expect(move.kind === 'share_via_host' && move.host).toMatchObject({ hostname: 'testnas.local', port: 445 })
  })
})
