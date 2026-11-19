import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { BarMenuView } from '#lib/ipc/bindings.js'
import { withPlatform } from '#lib/platform/test-platform.js'
import { fireWindowBarKeys } from './window-bar-keys'

const BAR: BarMenuView[] = [
  {
    id: 'menu_tab',
    title: { text: 'Tab' },
    rows: [
      {
        kind: 'item',
        id: 'close_tab',
        label: { text: 'Close tab' },
        shortcut: 'Ctrl+W',
        ariaKeyshortcuts: 'Control+W',
        enabled: true,
      },
    ],
  },
]

function ctrlW(): KeyboardEvent {
  return new KeyboardEvent('keydown', { key: 'w', code: 'KeyW', ctrlKey: true, bubbles: true, cancelable: true })
}

describe('fireWindowBarKeys', () => {
  let undo: () => void
  let stop: (() => void) | null = null
  const unsubscribe = vi.fn()
  const pick = vi.fn()

  beforeEach(() => {
    undo = withPlatform('linux')
    pick.mockClear()
    unsubscribe.mockClear()
  })
  afterEach(() => {
    stop?.()
    stop = null
    undo()
  })

  // Regression anchor (review of #1265): off macOS Settings, the queue, Keyboard shortcuts, and
  // Debug lost Ctrl+W with the native bar, and nothing else ran it.
  it('picks the row whose key a person pressed, and claims the key', async () => {
    stop = fireWindowBarKeys({ follow: (onMenus) => (onMenus(BAR), Promise.resolve(unsubscribe)), pick })
    await Promise.resolve()
    const event = ctrlW()
    document.body.dispatchEvent(event)
    expect(pick).toHaveBeenCalledWith('close_tab')
    expect(event.defaultPrevented).toBe(true)
  })

  it('leaves a key the page took to the page', async () => {
    stop = fireWindowBarKeys({ follow: (onMenus) => (onMenus(BAR), Promise.resolve(unsubscribe)), pick })
    await Promise.resolve()
    const event = ctrlW()
    event.preventDefault()
    document.body.dispatchEvent(event)
    expect(pick).not.toHaveBeenCalled()
  })

  it('stops listening and following on teardown, even before the follow resolved', async () => {
    let resolve: (stop: () => void) => void = () => {}
    const teardown = fireWindowBarKeys({
      follow: (onMenus) => {
        onMenus(BAR)
        return new Promise((done) => {
          resolve = done
        })
      },
      pick,
    })
    teardown()
    resolve(unsubscribe)
    await Promise.resolve()
    document.body.dispatchEvent(ctrlW())
    expect(pick).not.toHaveBeenCalled()
    expect(unsubscribe).toHaveBeenCalled()
  })
})
