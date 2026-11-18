/**
 * Return in the trash and delete dialog never moves anything to the trash, or deletes it, from a
 * footer button other than the confirm one, on any desktop (`#lib/ui/DETAILS.md` § Platform
 * conventions). Regression anchor: on the Mac, Return on a focused Cancel once moved the
 * selection to the trash.
 */

import { describe, expect, it, onTestFinished, vi } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import DeleteDialog from './DeleteDialog.svelte'
import { withConventions } from '#lib/platform/test-platform.js'
import { ALL_CONVENTION_DESKTOPS, pressReturnOnFooterButtons } from '#lib/ui/test-return-safety.js'

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

const ITEMS = [{ name: 'notes.txt', isDirectory: false, isSymlink: false, size: 12 }]

async function mountDialog(
  isPermanent: boolean,
): Promise<{ target: HTMLElement; onConfirm: ReturnType<typeof vi.fn> }> {
  const target = document.createElement('div')
  document.body.appendChild(target)
  const onConfirm = vi.fn()
  const dialog = mount(DeleteDialog, {
    target,
    props: {
      sourceItems: ITEMS,
      sourcePaths: ['/Users/test/notes.txt'],
      sourceFolderPath: '/Users/test',
      isPermanent,
      supportsTrash: true,
      isFromCursor: true,
      sourceVolumeId: 'root',
      onConfirm,
      onCancel: () => {},
    },
  })
  onTestFinished(() => {
    void unmount(dialog)
    target.remove()
  })
  await tick()
  return { target, onConfirm }
}

describe.each(ALL_CONVENTION_DESKTOPS)('the delete dialog on %s', (desktop) => {
  it.each([
    ['trash', false],
    ['permanent delete', true],
  ] as const)('never confirms a %s from Return on another footer button', async (_label, isPermanent) => {
    onTestFinished(withConventions(desktop))
    const { target, onConfirm } = await mountDialog(isPermanent)
    const confirm = target.querySelector<HTMLButtonElement>('[data-dialog-default]')
    expect(confirm).not.toBeNull()

    expect(pressReturnOnFooterButtons(target, (button) => button === confirm)).toBeGreaterThan(0)
    await tick()
    expect(onConfirm).not.toHaveBeenCalled()

    // The control: Return from the dialog itself still confirms, as it did before.
    const panel = target.querySelector<HTMLElement>('[role="dialog"], [role="alertdialog"]')
    panel?.focus()
    panel?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    await vi.waitFor(() => {
      expect(onConfirm).toHaveBeenCalledTimes(1)
    })
  })
})
