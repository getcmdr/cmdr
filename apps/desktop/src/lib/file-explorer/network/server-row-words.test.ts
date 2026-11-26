/**
 * The words and the dot a Servers row and a saved share's row carry, from Rust's facts.
 */
import { describe, it, expect } from 'vitest'
import type { FileEntry } from '../types'
import type { ServerMeta } from '#lib/ipc/bindings.js'
import { rowStatusBadge, rowStatusWords, wordServerDetails } from './server-row-words'

const meta = (patch: Partial<ServerMeta>): ServerMeta => ({
  rowId: 'r',
  protocol: 'smb',
  address: '192.168.1.111',
  status: 'saved',
  account: null,
  saved: true,
  place: null,
  pinned: false,
  ...patch,
})

const row = (patch: Partial<FileEntry>) =>
  ({ name: 'x', path: 'smb://x', isDirectory: true, isSymlink: false, iconId: 'dir', ...patch }) as FileEntry

describe('wordServerDetails', () => {
  it('says the protocol and the address, and the account when one is known', () => {
    expect(wordServerDetails(meta({}))).toBe('SMB · 192.168.1.111')
    expect(wordServerDetails(meta({ account: { kind: 'user', username: 'test_user_1' } }))).toBe(
      'SMB · 192.168.1.111 · as test_user_1',
    )
    expect(wordServerDetails(meta({ account: { kind: 'guest' } }))).toBe('SMB · 192.168.1.111 · as guest')
    expect(wordServerDetails(meta({ protocol: 'webdav', address: 'dav.testnas.local' }))).toBe(
      'WebDAV · dav.testnas.local',
    )
  })
})

describe('rowStatusBadge', () => {
  it('draws each status in the connection dot’s colors, with its words', () => {
    expect(rowStatusBadge(row({ serverMeta: meta({ status: 'connected' }) }))).toEqual({
      state: 'direct',
      label: 'Connected',
    })
    expect(rowStatusBadge(row({ serverMeta: meta({ status: 'signed_out' }) }))?.state).toBe('needs_sign_in')
    expect(rowStatusBadge(row({ serverMeta: meta({ status: 'waiting_for_key' }) }))?.state).toBe(
      'needs_host_key_approval',
    )
    expect(rowStatusBadge(row({ serverMeta: meta({ status: 'not_connected_yet' }) }))?.state).toBe('saved')
  })

  it('draws no dot for a host Cmdr only found, yet says its status', () => {
    const found = row({ serverMeta: meta({ status: 'found_nearby', saved: false }) })
    expect(rowStatusBadge(found)).toBeNull()
    expect(rowStatusWords(found)).toBe('Found nearby')
  })

  it('draws a saved share’s dot, and none on a share nobody saved or a plain folder', () => {
    const saved = row({
      shareMeta: { description: null, saved: { volumeId: 'v', status: 'connected', account: null, pinned: false } },
    })
    expect(rowStatusBadge(saved)?.state).toBe('direct')
    expect(rowStatusBadge(row({ shareMeta: { description: null, saved: null } }))).toBeNull()
    expect(rowStatusBadge(row({}))).toBeNull()
    expect(rowStatusWords(row({}))).toBeNull()
  })
})
