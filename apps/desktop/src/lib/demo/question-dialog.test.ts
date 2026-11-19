import { afterEach, describe, expect, it, vi } from 'vitest'
import { tick } from 'svelte'
import { ask, confirm, message } from '@tauri-apps/plugin-dialog'
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { askQuestion, questionOf } from './question-dialog.js'

vi.mock('#lib/tauri-commands/index.js', () => ({
  notifyDialogOpened: vi.fn(() => Promise.resolve()),
  notifyDialogClosed: vi.fn(() => Promise.resolve()),
}))

/** Routes the real plugin APIs' IPC to the demo's answer, as a webview does. */
function wire() {
  mockIPC((cmd, payload) => {
    if (cmd !== 'plugin:dialog|message') throw new Error(`no answer for ${cmd}`)
    return askQuestion((payload ?? {}) as Record<string, unknown>)
  })
}

/** The open question's buttons, by their words, once it renders. */
async function buttons(): Promise<HTMLButtonElement[]> {
  await tick()
  await tick()
  return [...document.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] .modal-footer-actions button')]
}

async function click(word: string): Promise<void> {
  const button = (await buttons()).find((candidate) => candidate.textContent.trim() === word)
  if (!button) throw new Error(`no ${word} button`)
  button.click()
  await tick()
}

afterEach(() => {
  clearMocks()
  vi.unstubAllGlobals()
  document.body.innerHTML = ''
})

describe('questionOf', () => {
  it('lays a custom two-button set out with the confirming one last and default, and Escape cancelling', () => {
    const question = questionOf(
      { message: 'Forget it?', title: 'Forget', buttons: { OkCancelCustom: ['Forget', 'Keep'] } },
      'Cmdr',
    )
    expect(question).toEqual({
      title: 'Forget',
      message: 'Forget it?',
      buttons: [
        { label: 'Keep', answer: 'Keep', isDefault: false },
        { label: 'Forget', answer: 'Forget', isDefault: true },
      ],
      escapeAnswer: 'Keep',
    })
  })

  it('keeps every button of a three-button set, and Escape answers Cancel', () => {
    const question = questionOf({ message: 'Save?', buttons: 'YesNoCancel' }, 'Cmdr')
    expect(question.buttons.map((button) => button.answer)).toEqual(['Cancel', 'No', 'Yes'])
    expect(question.buttons.map((button) => button.label)).toEqual(['Cancel', 'No', 'Yes'])
    expect(question.escapeAnswer).toBe('Cancel')
    expect(question.title).toBe('Cmdr')
  })

  it('answers a lone OK with OK, Escape included', () => {
    const question = questionOf({ message: 'Done' }, 'Cmdr')
    expect(question.buttons).toEqual([{ label: 'OK', answer: 'Ok', isDefault: true }])
    expect(question.escapeAnswer).toBe('Ok')
  })
})

describe("the demo's question, in the app's own dialog", () => {
  it('answers `ask` with custom labels by the clicked button, without blocking the page', async () => {
    wire()
    const confirmSpy = vi.fn(() => true)
    vi.stubGlobal('confirm', confirmSpy)
    const answer = ask('Forget it?', { title: 'Forget', okLabel: 'Forget', cancelLabel: 'Cancel' })
    expect((await buttons()).map((button) => button.textContent.trim())).toEqual(['Cancel', 'Forget'])
    expect(document.querySelector('[role="alertdialog"]')?.textContent).toContain('Forget it?')
    await click('Forget')
    await expect(answer).resolves.toBe(true)
    expect(confirmSpy).not.toHaveBeenCalled()
    expect(document.querySelector('[role="alertdialog"]')).toBeNull()
  })

  it('answers No for a plain Yes/No question when No is clicked', async () => {
    wire()
    const answer = ask('Sure?')
    await click('No')
    await expect(answer).resolves.toBe(false)
  })

  it('answers each button of a Yes/No/Cancel set', async () => {
    for (const word of ['Yes', 'No', 'Cancel']) {
      wire()
      const answer = askQuestion({ message: 'Save changes?', buttons: 'YesNoCancel' })
      await click(word)
      await expect(answer).resolves.toBe(word)
      clearMocks()
    }
  })

  it('answers Cancel on Escape, as the native alert does', async () => {
    wire()
    const answer = confirm('Sure?')
    await buttons()
    document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await tick()
    await expect(answer).resolves.toBe(false)
  })

  it('shows a lone message with one OK', async () => {
    wire()
    const answer = message('Done')
    expect((await buttons()).map((button) => button.textContent.trim())).toEqual(['OK'])
    await click('OK')
    await expect(answer).resolves.toBe('Ok')
  })
})
