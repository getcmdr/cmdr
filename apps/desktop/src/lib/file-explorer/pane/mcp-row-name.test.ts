/**
 * What a row's name says in `cmdr://state`: an agent reads the facts the row shows as
 * locale-independent tokens, ❌ never the translated words.
 */
import { describe, it, expect } from 'vitest'
import type { FileEntry } from '../types'
import { mcpRowName } from './mcp-row-name'

const base = { path: 'smb://testnas', isDirectory: true, isSymlink: false, iconId: 'dir' } as FileEntry

describe('mcpRowName', () => {
  it('keeps a plain row’s name', () => {
    expect(mcpRowName({ ...base, name: 'Documents' })).toBe('Documents')
  })

  it('gives a share row its description as a comment', () => {
    expect(mcpRowName({ ...base, name: 'media', shareMeta: { description: 'Films', saved: null } })).toBe(
      'media  comment="Films"',
    )
  })

  it('gives a saved share its status and account', () => {
    const entry = {
      ...base,
      name: 'archive',
      shareMeta: {
        description: null,
        saved: {
          volumeId: 'v',
          status: 'connected',
          account: { kind: 'user', username: 'test_user_1' },
          pinned: true,
        },
      },
    } as FileEntry
    expect(mcpRowName(entry)).toBe('archive  status=connected account=test_user_1')
  })

  it('gives a Servers row its protocol, status, address, and account', () => {
    const entry = {
      ...base,
      name: 'Naspolya',
      serverMeta: {
        rowId: 'r',
        protocol: 'smb',
        address: '192.168.1.111',
        status: 'found_nearby',
        account: { kind: 'guest' },
        saved: false,
        place: null,
        pinned: false,
      },
    } as FileEntry
    expect(mcpRowName(entry)).toBe('Naspolya  protocol=smb status=found_nearby address=192.168.1.111 account=guest')
  })

  it('leaves the account out when nothing says it', () => {
    const entry = {
      ...base,
      name: 'Testhost',
      serverMeta: {
        rowId: 'r',
        protocol: 'sftp',
        address: 'testhost:22',
        status: 'saved',
        account: null,
        saved: true,
        place: 'p',
        pinned: false,
      },
    } as FileEntry
    expect(mcpRowName(entry)).toBe('Testhost  protocol=sftp status=saved address=testhost:22')
  })
})
