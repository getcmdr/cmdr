/**
 * The `..` explainer fires exactly once per install, whatever share or pane leads out next.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const { addToast, settings } = vi.hoisted(() => ({
  addToast: vi.fn(),
  settings: new Map<string, unknown>(),
}))
vi.mock('#lib/ui/toast/index.js', () => ({ addToast }))
vi.mock('#lib/settings/index.js', () => ({
  getSetting: (id: string) => settings.get(id) ?? false,
  setSetting: (id: string, value: unknown) => settings.set(id, value),
}))
vi.mock('./ShareParentHintToastContent.svelte', () => ({ default: 'ShareParentHintToastContent' }))

import { showShareParentHintOnce } from './share-parent-hint'

const landing = { volumeId: 'smb-testnas-media', hostId: 'testnas', shareName: 'media', serverLabel: 'Testnas' }

beforeEach(() => {
  addToast.mockClear()
  settings.clear()
})

describe('showShareParentHintOnce', () => {
  it('raises the sticky explainer the first time, naming the share and the server', () => {
    showShareParentHintOnce(landing)
    expect(addToast).toHaveBeenCalledOnce()
    expect(addToast.mock.calls[0][1]).toMatchObject({
      dismissal: 'persistent',
      props: { volumeId: 'smb-testnas-media', serverLabel: 'Testnas' },
    })
    expect(settings.get('behavior.shareParentRowHintSeen')).toBe(true)
  })

  it('never raises it again, on any share', () => {
    showShareParentHintOnce(landing)
    showShareParentHintOnce({ ...landing, volumeId: 'smb-testnas-backup', shareName: 'backup' })
    expect(addToast).toHaveBeenCalledOnce()
  })
})
