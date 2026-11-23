/**
 * `DeleteDialog.svelte`: a link to a folder (a Windows junction, a symlinked folder)
 * lists as a folder, but only the link goes. Its row reads as the one item the
 * delete removes, the way the title counts it (#1432): a file's icon and the link's
 * own size, ❌ never the target's recursive size or file count.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, tick } from 'svelte'
import DeleteDialog from './DeleteDialog.svelte'
import type { DeleteSourceItem } from './delete-dialog-utils'

vi.mock('#lib/tauri-commands/index.js', () => ({
  estimateOperationCost: vi.fn(() => Promise.resolve([])),
  notifyDialogOpened: vi.fn(() => Promise.resolve()),
  notifyDialogClosed: vi.fn(() => Promise.resolve()),
  startScanPreview: vi.fn(() => Promise.resolve({ previewId: 'preview-1' })),
  cancelScanPreview: vi.fn(() => Promise.resolve()),
  onScanPreviewProgress: vi.fn(() => Promise.resolve(() => {})),
  onScanPreviewComplete: vi.fn(() => Promise.resolve(() => {})),
  onScanPreviewError: vi.fn(() => Promise.resolve(() => {})),
  onScanPreviewCancelled: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('#lib/settings/index.js', () => ({
  getSetting: vi.fn(() => 500),
}))

vi.mock('#lib/settings/reactive-settings.svelte.js', () => ({
  formatFileSize: vi.fn((n: number | undefined) => (n === undefined ? '' : `${String(n)} B`)),
  getFileSizeFormat: vi.fn(() => 'binary'),
  getFileSizeUnit: vi.fn(() => 'bytes'),
}))

const targetFolder = {
  isDirectory: true,
  recursiveSize: 12_288,
  recursiveSizeComplete: true,
  recursiveFileCount: 3,
}

/** The one row the dialog renders for `item`: its icon markup and its size cell's text. */
async function row(item: DeleteSourceItem): Promise<{ icon: string; size: string }> {
  const target = document.createElement('div')
  document.body.appendChild(target)
  mount(DeleteDialog, {
    target,
    props: {
      sourceItems: [item],
      sourcePaths: ['C:/Users/test_user_1/w1123/test_junction_1'],
      sourceFolderPath: 'C:/Users/test_user_1/w1123',
      isPermanent: true,
      supportsTrash: false,
      isFromCursor: true,
      sourceVolumeId: 'root',
      onConfirm: () => {},
      onCancel: () => {},
    },
  })
  await tick()
  const icon = target.querySelector('.item-icon')?.innerHTML ?? ''
  const size = target.querySelector('.item-size')?.textContent ?? ''
  target.remove()
  return { icon, size }
}

describe('DeleteDialog link-to-a-folder row', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
  })
  afterEach(() => {
    document.body.innerHTML = ''
  })

  it('reads as the one link it deletes, never as its target folder', async () => {
    const link = await row({ name: 'test_junction_1', size: 0, isSymlink: true, ...targetFolder })
    const folder = await row({ name: 'Photos', isSymlink: false, ...targetFolder })
    const file = await row({ name: 'a.txt', size: 0, isDirectory: false, isSymlink: false })

    expect(link.icon, 'a file icon').toBe(file.icon)
    expect(link.icon).not.toBe(folder.icon)
    expect(link.size, "the link's own size, not the target's").toBe(file.size)
    expect(link.size).not.toBe(folder.size)
    expect(link.size, "not the target's file count").not.toMatch(/3\s+files?/)
    // The plain folder still reads its target-side count, so the difference is the link.
    expect(folder.size).toMatch(/3\s+files?/)
  })
})
