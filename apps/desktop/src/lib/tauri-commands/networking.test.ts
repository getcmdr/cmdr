/**
 * Where `..` out of an SMB share leads arrives two ways (the `get_smb_share_parent`
 * command and a top-folder listing's `listing-complete`), both in the wire's shape. One
 * conversion turns it into the frontend's, so the two can't disagree.
 */

import { describe, it, expect, vi } from 'vitest'

vi.mock('#lib/ipc/bindings.js', () => ({ commands: { getSmbShareParent: vi.fn() } }))
vi.mock('#lib/logging/logger.js', () => ({
  getAppLogger: () => ({ warn: vi.fn(), info: vi.fn(), debug: vi.fn(), error: vi.fn() }),
}))

import { commands, type ShareParent as ShareParentWire } from '#lib/ipc/bindings.js'
import { getSmbShareParent, shareParentFromWire } from './networking'

function wire(hostname: string | null, ipAddress: string | null): ShareParentWire {
  return {
    host: { id: 'testnas', name: 'Testnas', hostname, ipAddress, port: 445 },
    serverLabel: 'Testnas',
    shareName: 'media',
    showParentRow: null,
  }
}

describe('shareParentFromWire', () => {
  it('leaves out a host name and address the wire spells `null`', () => {
    const parent = shareParentFromWire(wire(null, null))

    expect(parent.host.hostname).toBeUndefined()
    expect(parent.host.ipAddress).toBeUndefined()
    expect(parent.shareName).toBe('media')
  })

  it('keeps a resolved host name and address', () => {
    const parent = shareParentFromWire(wire('testnas.local', '192.0.2.10'))

    expect(parent.host).toMatchObject({ hostname: 'testnas.local', ipAddress: '192.0.2.10', port: 445 })
  })
})

describe('getSmbShareParent', () => {
  it('converts what the command answers', async () => {
    vi.mocked(commands.getSmbShareParent).mockResolvedValueOnce(wire(null, '192.0.2.10'))

    const parent = await getSmbShareParent('smb-testnas-media')

    expect(parent?.host).toMatchObject({ hostname: undefined, ipAddress: '192.0.2.10' })
  })

  it('answers `null` when no SMB share is behind the volume', async () => {
    vi.mocked(commands.getSmbShareParent).mockResolvedValueOnce(null)

    expect(await getSmbShareParent('root')).toBeNull()
  })
})
