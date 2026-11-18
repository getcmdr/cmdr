/**
 * The New folder / New filename check.
 *
 * ❗ Only the LATEST lookup's answer counts. A validate on open and another from a
 * directory diff overlapped, and the older one landing last showed "There is
 * already a folder by this name" for a name that was free (QA round 5).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const { findFileIndex, getFileAt } = vi.hoisted(() => ({ findFileIndex: vi.fn(), getFileAt: vi.fn() }))
vi.mock('#lib/tauri-commands/index.js', () => ({ findFileIndex, getFileAt, onDirectoryDiff: vi.fn() }))

import { NewEntryNameCheck } from './new-entry-name-check.svelte'

describe('NewEntryNameCheck', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    getFileAt.mockResolvedValue({ isDirectory: true })
  })

  it('keeps the latest lookup’s answer when an older one lands after it', async () => {
    let answerOld!: (index: number | null) => void
    findFileIndex
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            answerOld = resolve
          }),
      )
      .mockResolvedValueOnce(null)
    const check = new NewEntryNameCheck({
      currentPath: '/Volumes/public',
      listingId: 'l1',
      showHiddenFiles: false,
      getName: () => 'qa-probe',
    })

    const older = check.validate('qa-probe')
    await check.validate('qa-probe')
    answerOld(3)
    await older

    expect(check.errorMessage).toBe('')
    expect(check.isChecking).toBe(false)
  })

  it('still reports a real clash', async () => {
    findFileIndex.mockResolvedValue(0)
    const check = new NewEntryNameCheck({
      currentPath: '/x',
      listingId: 'l1',
      showHiddenFiles: false,
      getName: () => 'docs',
    })
    await check.validate('docs')
    expect(check.errorMessage).not.toBe('')
  })
})
