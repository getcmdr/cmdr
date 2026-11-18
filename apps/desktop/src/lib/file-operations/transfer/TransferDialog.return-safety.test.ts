/**
 * Return in the copy and move dialog never starts a transfer from a footer button other than the
 * confirm one, on any desktop (`#lib/ui/DETAILS.md` § Platform conventions). Regression anchor:
 * on the Mac, Return on a focused Cancel once started a Move with "overwrite all".
 */

import { describe, it, expect, vi, onTestFinished } from 'vitest'
import { withConventions } from '#lib/platform/test-platform.js'
import { ALL_CONVENTION_DESKTOPS, pressReturnOnFooterButtons } from '#lib/ui/test-return-safety.js'
import {
  confirmButton,
  mountDialog,
  pathInput,
  settleExistsCheck,
  flushMicrotasks,
  type ConfirmFn,
} from './test-transfer-dialog-harness'

describe.each(ALL_CONVENTION_DESKTOPS)('the transfer dialog on %s', (desktop) => {
  it.each(['copy', 'move'] as const)(
    'never starts a %s from Return on another footer button',
    async (operationType) => {
      onTestFinished(withConventions(desktop))
      const onConfirm = vi.fn<ConfirmFn>()
      const target = mountDialog({ operationType, presetOnConflict: 'overwrite_all', onConfirm })
      await settleExistsCheck()

      const confirm = confirmButton(target)
      expect(pressReturnOnFooterButtons(target, (button) => button === confirm)).toBeGreaterThan(0)
      await flushMicrotasks()
      expect(onConfirm).not.toHaveBeenCalled()

      // The control: Return from the path field still starts it, as it did before.
      pathInput(target).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
      await flushMicrotasks()
      expect(onConfirm).toHaveBeenCalledTimes(1)
    },
  )
})
