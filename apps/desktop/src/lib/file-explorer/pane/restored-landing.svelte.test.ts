/**
 * A restored Servers tab's landing: the share-list path whose Servers row its first listing
 * puts the cursor on. Spent once, so a later listing of the Servers list lands nowhere special.
 */
import { describe, it, expect, vi } from 'vitest'

const h = vi.hoisted(() => ({
  tabs: {
    left: { landOnServer: undefined as string | undefined },
    right: { landOnServer: undefined as string | undefined },
  },
}))

vi.mock('./explorer-state.svelte', () => ({
  explorerState: { getTabMgr: (paneId: 'left' | 'right') => paneId },
}))
vi.mock('../tabs/tab-state-manager.svelte', () => ({
  getActiveTab: (manager: 'left' | 'right') => h.tabs[manager],
}))

import { takeRestoredLanding } from './restored-landing.svelte'

describe('takeRestoredLanding', () => {
  it('hands the pane its tab’s landing once, then nothing', () => {
    h.tabs.left.landOnServer = 'smb://testnas'
    expect(takeRestoredLanding('left')).toBe('smb://testnas')
    expect(takeRestoredLanding('left')).toBeUndefined()
  })

  it('takes only its own pane’s landing', () => {
    h.tabs.right.landOnServer = 'smb://testnas2'
    expect(takeRestoredLanding('left')).toBeUndefined()
    expect(h.tabs.right.landOnServer).toBe('smb://testnas2')
  })
})
