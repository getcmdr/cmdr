/**
 * "Show in Finder / File Explorer": every refusal ends in a worded toast, whether it came before
 * the file manager was asked or from the file manager itself, and a shown item says nothing.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { RevealError } from '#lib/ipc/bindings.js'

const m = vi.hoisted(() => ({
  showInFinder: vi.fn<(path: string) => Promise<RevealError | null>>(),
  addToast: vi.fn(),
}))

vi.mock('#lib/tauri-commands/index.js', () => ({ showInFinder: m.showInFinder }))
vi.mock('#lib/ui/toast/index.js', () => ({ addToast: m.addToast }))
vi.mock(import('#lib/intl/messages.svelte.js'), async (importOriginal) => ({
  ...(await importOriginal()),
  tString: (key: string) => key,
}))

import { showInFileManagerOrExplain } from './file-manager-reveal'

beforeEach(() => {
  m.showInFinder.mockReset()
  m.addToast.mockReset()
})

describe('showInFileManagerOrExplain', () => {
  it('says nothing when the file manager was asked', async () => {
    m.showInFinder.mockResolvedValue(null)

    await expect(showInFileManagerOrExplain('/Users/test_user_10/notes.txt')).resolves.toBe(true)

    expect(m.addToast).not.toHaveBeenCalled()
  })

  it.each([
    ['noPath', 'fileExplorer.reveal.noPath'],
    ['noPlainPath', 'fileExplorer.reveal.noPlainPath'],
    ['notShown', 'fileExplorer.reveal.notShown'],
  ] as const)('words a %s refusal as a warning toast', async (type, key) => {
    m.showInFinder.mockResolvedValue({ type })

    await expect(showInFileManagerOrExplain('/Users/test_user_10/notes.txt')).resolves.toBe(false)

    expect(m.addToast).toHaveBeenCalledExactlyOnceWith(key, expect.objectContaining({ level: 'warn' }))
  })

  it('treats a broken bridge as not shown, never an unhandled throw', async () => {
    m.showInFinder.mockRejectedValue(new Error('bridge gone'))

    await expect(showInFileManagerOrExplain('/Users/test_user_10/notes.txt')).resolves.toBe(false)

    expect(m.addToast).toHaveBeenCalledExactlyOnceWith(
      'fileExplorer.reveal.notShown',
      expect.objectContaining({ level: 'warn' }),
    )
  })
})
