/**
 * ❗ cmdr-reports#7: "Cmdr tried to open the share on that server." What a door queues for
 * a share list (a share to mount, the share `..` came out of, the one ask) is a one-shot
 * for the visit it was queued with.
 */
import { describe, it, expect, beforeEach } from 'vitest'
import {
  claimShareListAsk,
  dropShareListArrivalOff,
  openShareListArrival,
  queueShareListArrival,
  shareListArrivalFor,
  takeShareListArrival,
} from './share-list-arrival.svelte'
import type { NetworkHost } from '../types'

const testnas: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'testnas.local',
  port: 445,
  source: 'discovered',
}

beforeEach(() => {
  // Spend whatever a previous test left on either pane.
  dropShareListArrivalOff('left', 'nowhere')
  dropShareListArrivalOff('right', 'nowhere')
})

describe('a share list’s arrival', () => {
  it('is queued for the server’s share-list path, and spent once the list lands there', () => {
    const path = openShareListArrival('left', testnas, { origin: 'smb://', mount: 'public' })
    expect(path).toBe('smb://testnas')

    expect(takeShareListArrival('left', 'smb://testnas')).toMatchObject({ mount: 'public' })
    // A list that comes back (after a Cancel, a refused mount) finds nothing queued.
    expect(takeShareListArrival('left', 'smb://testnas')).toBeNull()
  })

  it('is kept per pane, and only for the share list it was queued for', () => {
    openShareListArrival('left', testnas, { origin: 'smb://', mount: 'public' })
    expect(takeShareListArrival('right', 'smb://testnas')).toBeNull()
    expect(takeShareListArrival('left', 'smb://attic')).toBeNull()
    expect(shareListArrivalFor('left')).not.toBeNull()
  })

  it('survives the pane sitting where it was queued from or for, and drops anywhere else', () => {
    openShareListArrival('left', testnas, { origin: 'smb://', mount: 'public' })
    dropShareListArrivalOff('left', 'smb://')
    dropShareListArrivalOff('left', 'smb://testnas')
    expect(shareListArrivalFor('left')).not.toBeNull()

    // Back, another server, or a refused navigation that left the pane elsewhere.
    dropShareListArrivalOff('left', 'smb://attic')
    expect(shareListArrivalFor('left')).toBeNull()
  })

  it('lets a sign-in refusal ask once, and only on the share list it was queued for', () => {
    queueShareListArrival('left', { path: 'smb://testnas', origin: 'smb://' })
    expect(claimShareListAsk('left', 'smb://attic')).toBe(false)
    expect(claimShareListAsk('left', 'smb://testnas')).toBe(true)
    expect(claimShareListAsk('left', 'smb://testnas'), 'a repaint asks nothing').toBe(false)
  })

  it('asks nothing on a share list no door sent the pane to (a tab coming back into view)', () => {
    expect(claimShareListAsk('left', 'smb://testnas')).toBe(false)
  })
})
