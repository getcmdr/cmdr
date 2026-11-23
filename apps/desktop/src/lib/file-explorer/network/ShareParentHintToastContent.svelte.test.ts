/**
 * The `..` explainer's three buttons: this share off, every share off, or just close.
 * Each one closes the toast.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushSync } from 'svelte'

const { dismissToast, setShareParentRow, turnOffShareParentRowForAll } = vi.hoisted(() => ({
  dismissToast: vi.fn(),
  setShareParentRow: vi.fn(() => Promise.resolve(true)),
  turnOffShareParentRowForAll: vi.fn(),
}))
vi.mock('#lib/ui/toast/index.js', () => ({ dismissToast }))
vi.mock('./share-parent-row.svelte', () => ({ setShareParentRow, turnOffShareParentRowForAll }))

import ShareParentHintToastContent from './ShareParentHintToastContent.svelte'

function render() {
  const target = document.createElement('div')
  document.body.appendChild(target)
  mount(ShareParentHintToastContent, {
    target,
    props: { toastId: 'share-parent-row-hint', volumeId: 'smb-testnas-media', serverLabel: 'Testnas' },
  })
  flushSync()
  const button = (label: string) => {
    const found = Array.from(target.querySelectorAll('button')).find((b) => b.textContent.trim() === label)
    if (!found) throw new Error(`no “${label}” button`)
    return found
  }
  return { target, button }
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('ShareParentHintToastContent', () => {
  it('names the server it took the person to', () => {
    expect(render().target.textContent).toContain('“..” took you to Testnas’s shares.')
  })

  it('writes the buttons dismissive first, primary last', () => {
    const labels = Array.from(render().target.querySelectorAll('button')).map((b) => b.textContent.trim())
    expect(labels).toEqual(['Turn off for this share', 'Turn off for all shares', 'Got it'])
  })

  it('“Turn off for this share” turns it off for that share only', () => {
    render().button('Turn off for this share').click()
    expect(setShareParentRow).toHaveBeenCalledWith('smb-testnas-media', false)
    expect(turnOffShareParentRowForAll).not.toHaveBeenCalled()
    expect(dismissToast).toHaveBeenCalledWith('share-parent-row-hint')
  })

  it('“Turn off for all shares” turns the default off', () => {
    render().button('Turn off for all shares').click()
    expect(turnOffShareParentRowForAll).toHaveBeenCalledOnce()
    expect(setShareParentRow).not.toHaveBeenCalled()
  })

  it('“Got it” only closes it', () => {
    render().button('Got it').click()
    expect(dismissToast).toHaveBeenCalledWith('share-parent-row-hint')
    expect(setShareParentRow).not.toHaveBeenCalled()
    expect(turnOffShareParentRowForAll).not.toHaveBeenCalled()
  })
})
