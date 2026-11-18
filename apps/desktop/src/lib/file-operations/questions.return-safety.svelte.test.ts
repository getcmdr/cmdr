/**
 * The questions with no default button (the rollback question, locked sources) answer nothing
 * from Return on a footer button by themselves, on any desktop (`#lib/ui/DETAILS.md` § Platform
 * conventions). Where Return presses the focused button (Windows, Linux) the browser presses it,
 * as it did before the Windows port; happy-dom never simulates that, so a call here would be the
 * dialog's own doing.
 */

import { describe, expect, it, onTestFinished, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import type { OperationSnapshot } from '#lib/tauri-commands/index.js'
import { withConventions } from '#lib/platform/test-platform.js'
import { ALL_CONVENTION_DESKTOPS, pressReturnOnFooterButtons } from '#lib/ui/test-return-safety.js'
import RollbackConfirmDialog from './RollbackConfirmDialog.svelte'
import LockedSourcesDialog from './LockedSourcesDialog.svelte'

vi.mock('#lib/tauri-commands/index.js', () => ({
  notifyDialogOpened: vi.fn(() => Promise.resolve()),
  notifyDialogClosed: vi.fn(() => Promise.resolve()),
}))

const snapshot: OperationSnapshot = {
  operationId: 'op-1',
  operationType: 'move',
  status: 'running',
  source: 'Docs (7 items)',
  destination: 'Archive',
  rollbackSupport: 'crossVolumeMove',
  reverses: null,
  error: null,
  canRetry: false,
  conflictPolicy: null,
  startedAt: null,
  items: null,
}

function freshTarget(): HTMLElement {
  const target = document.createElement('div')
  document.body.appendChild(target)
  onTestFinished(() => {
    target.remove()
  })
  return target
}

describe.each(ALL_CONVENTION_DESKTOPS)('Return on a footer button on %s', (desktop) => {
  it.each(['stopAndDelete', 'stopAndMoveBack'] as const)('never rolls back (%s)', async (variant) => {
    onTestFinished(withConventions(desktop))
    const target = freshTarget()
    const onConfirm = vi.fn()
    const onCancel = vi.fn()
    const dialog = mount(RollbackConfirmDialog, { target, props: { variant, onConfirm, onCancel } })
    onTestFinished(() => void unmount(dialog))
    await tick()
    expect(pressReturnOnFooterButtons(target, () => false)).toBeGreaterThan(1)
    await tick()
    expect(onConfirm).not.toHaveBeenCalled()
  })

  it('never moves locked items anyway', async () => {
    onTestFinished(withConventions(desktop))
    const target = freshTarget()
    const onMoveAnyway = vi.fn()
    const onSkip = vi.fn()
    const dialog = mount(LockedSourcesDialog, {
      target,
      props: {
        event: { operationId: 'op-1', operationType: 'move', lockedCount: 1, lockedSample: ['/Docs/a.pdf'] },
        snapshot,
        answering: false,
        onMoveAnyway,
        onSkip,
        onCancel: () => {},
      },
    })
    onTestFinished(() => void unmount(dialog))
    flushSync()
    expect(pressReturnOnFooterButtons(target, () => false)).toBeGreaterThan(1)
    await tick()
    expect(onMoveAnyway).not.toHaveBeenCalled()
    expect(onSkip).not.toHaveBeenCalled()
  })
})
