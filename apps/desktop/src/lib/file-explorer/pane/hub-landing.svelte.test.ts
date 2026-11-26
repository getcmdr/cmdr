/**
 * The hub landing is derived at the one path write every commit goes through: leaving a
 * share list for the hub notes the server, and any other path change drops it.
 */
import { describe, it, expect } from 'vitest'
import type { TabState } from '../tabs/tab-types'
import { writeTabPath } from './hub-landing.svelte'

function tab(volumeId: string, path: string): TabState {
  return { volumeId, path } as TabState
}

describe('writeTabPath', () => {
  it('notes the server when a tab goes from its share list to the hub', () => {
    const t = tab('network', 'smb://testnas')
    writeTabPath(t, 'smb://')
    expect(t.path).toBe('smb://')
    expect(t.landOnServer).toBe('smb://testnas')
  })

  it('drops a pending landing on any other path change', () => {
    const t = tab('network', 'smb://')
    t.landOnServer = 'smb://testnas'
    writeTabPath(t, 'smb://testnas2')
    expect(t.landOnServer).toBeUndefined()
  })

  it('keeps a pending landing when the write leaves the path as it was', () => {
    // A restored share-list tab comes back as the hub with a landing; a re-commit of the
    // same `smb://` before the hub renders must not spend it.
    const t = tab('network', 'smb://')
    t.landOnServer = 'smb://testnas'
    writeTabPath(t, 'smb://')
    expect(t.landOnServer).toBe('smb://testnas')
  })

  it('notes nothing for a step that never was a share list', () => {
    const fromFolder = tab('root', '/Users/test_user_1')
    writeTabPath(fromFolder, 'smb://')
    expect(fromFolder.landOnServer).toBeUndefined()
    // A share's path names no share list: it's mounted, never listed.
    const fromShare = tab('network', 'smb://testnas/public')
    writeTabPath(fromShare, 'smb://')
    expect(fromShare.landOnServer).toBeUndefined()
  })
})
