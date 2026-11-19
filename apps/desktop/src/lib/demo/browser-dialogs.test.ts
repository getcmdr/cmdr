import { afterEach, describe, expect, it, vi } from 'vitest'
import { open, save } from '@tauri-apps/plugin-dialog'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { addToast } from '#lib/ui/toast/index.js'
import { browserDialogAnswers } from './browser-dialogs.js'

vi.mock('#lib/ui/toast/index.js', () => ({ addToast: vi.fn() }))

/** Routes the real plugin APIs' IPC to the demo's answers, as the backend does. */
function wire() {
  mockIPC((cmd, payload) => {
    const answer = browserDialogAnswers[cmd]
    if (!answer) throw new Error(`no answer for ${cmd}`)
    return answer((payload ?? {}) as Record<string, unknown>)
  })
}

afterEach(() => {
  clearMocks()
  vi.mocked(addToast).mockClear()
})

describe('the browser dialogs', () => {
  it('answers a picker as cancelled and tells the visitor the app does it', async () => {
    wire()
    await expect(open({ directory: true })).resolves.toBeNull()
    await expect(save({ defaultPath: 'notes.md' })).resolves.toBeNull()
    expect(vi.mocked(addToast)).toHaveBeenCalledTimes(2)
  })

  it('says the same for a reveal in the file manager', async () => {
    wire()
    await expect(revealItemInDir('/Users/you/logs')).resolves.toBeNull()
    expect(vi.mocked(addToast)).toHaveBeenCalledTimes(1)
  })
})
