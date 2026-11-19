/**
 * Tier-3 a11y for the demo's question dialog: a two-button and a three-button question, each an
 * `alertdialog` named by its title and described by its message.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import { expectNoA11yViolations } from '#lib/test-a11y'
import QuestionDialog from './QuestionDialog.svelte'
import { questionOf } from './question-dialog.js'

vi.mock('#lib/tauri-commands/index.js', () => ({
  notifyDialogOpened: vi.fn(() => Promise.resolve()),
  notifyDialogClosed: vi.fn(() => Promise.resolve()),
}))

let mounted: ReturnType<typeof mount> | undefined

afterEach(() => {
  if (mounted) void unmount(mounted)
  mounted = undefined
  document.body.innerHTML = ''
})

async function render(payload: Record<string, unknown>): Promise<HTMLElement> {
  const target = document.createElement('div')
  document.body.append(target)
  mounted = mount(QuestionDialog, { target, props: { question: questionOf(payload, 'Cmdr'), onAnswer: () => {} } })
  await tick()
  return target
}

describe('QuestionDialog a11y', () => {
  it('a custom two-button question has no violations', async () => {
    const target = await render({
      message: 'Forget it?',
      title: 'Forget',
      buttons: { OkCancelCustom: ['Forget', 'Cancel'] },
    })
    expect(document.querySelector('[role="alertdialog"]')).not.toBeNull()
    await expectNoA11yViolations(target)
  })

  it('a Yes/No/Cancel question has no violations', async () => {
    const target = await render({ message: 'Save changes?', buttons: 'YesNoCancel' })
    await expectNoA11yViolations(target)
  })
})
