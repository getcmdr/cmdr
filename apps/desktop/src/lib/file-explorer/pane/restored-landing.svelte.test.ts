/**
 * A tab's one-shot landing: the row its next listing of one folder puts the cursor on. Spent
 * once, and only by a listing of that folder.
 */
import { describe, it, expect, vi } from 'vitest'

type Landing = { path: string; selectPath: string } | undefined
const h = vi.hoisted(() => ({
  tabs: {
    left: { landOn: undefined as Landing },
    right: { landOn: undefined as Landing },
  },
}))

vi.mock('./explorer-state.svelte', () => ({
  explorerState: { getTabMgr: (paneId: 'left' | 'right') => paneId },
}))
vi.mock('../tabs/tab-state-manager.svelte', () => ({
  getActiveTab: (manager: 'left' | 'right') => h.tabs[manager],
}))

import { queueLanding, takeLanding } from './restored-landing.svelte'

describe('takeLanding', () => {
  it('hands the pane its tab’s landing once, then nothing', () => {
    queueLanding('left', { path: 'smb://', selectPath: 'smb://testnas' })
    expect(takeLanding('left', 'smb://')).toBe('smb://testnas')
    expect(takeLanding('left', 'smb://')).toBeUndefined()
  })

  it('takes only its own pane’s landing', () => {
    queueLanding('right', { path: 'smb://', selectPath: 'smb://testnas2' })
    expect(takeLanding('left', 'smb://')).toBeUndefined()
    expect(h.tabs.right.landOn?.selectPath).toBe('smb://testnas2')
  })

  it('leaves the landing to the listing of its own folder', () => {
    queueLanding('left', {
      path: 's3://test_key_1@s3.example.com:443/',
      selectPath: 's3://test_key_1@s3.example.com:443/b',
    })
    expect(takeLanding('left', '/Users/test_user_1')).toBeUndefined()
    expect(takeLanding('left', 's3://test_key_1@s3.example.com:443/')).toBe('s3://test_key_1@s3.example.com:443/b')
  })
})
