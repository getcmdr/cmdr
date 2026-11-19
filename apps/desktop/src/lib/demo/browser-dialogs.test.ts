import { afterEach, describe, expect, it, vi } from 'vitest'
import { ask, confirm, message, open, save } from '@tauri-apps/plugin-dialog'
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
  vi.unstubAllGlobals()
  vi.mocked(addToast).mockClear()
})

describe('the browser dialogs', () => {
  it('asks `ask` with custom labels in the browser’s confirm, and answers the confirming label', async () => {
    wire()
    const confirmSpy = vi.fn(() => true)
    vi.stubGlobal('confirm', confirmSpy)
    await expect(ask('Forget it?', { title: 'Forget', okLabel: 'Forget', cancelLabel: 'Cancel' })).resolves.toBe(true)
    expect(confirmSpy).toHaveBeenCalledWith('Forget\n\nForget it?')
    confirmSpy.mockReturnValue(false)
    await expect(ask('Forget it?', { okLabel: 'Forget', cancelLabel: 'Cancel' })).resolves.toBe(false)
  })

  it('answers the plain Yes/No and OK/Cancel sets too', async () => {
    wire()
    vi.stubGlobal(
      'confirm',
      vi.fn(() => true),
    )
    await expect(ask('Sure?')).resolves.toBe(true)
    await expect(confirm('Sure?')).resolves.toBe(true)
  })

  it('shows a lone message in the browser’s alert', async () => {
    wire()
    const alertSpy = vi.fn()
    vi.stubGlobal('alert', alertSpy)
    await expect(message('Done')).resolves.toBe('Ok')
    expect(alertSpy).toHaveBeenCalledWith('Done')
  })

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
