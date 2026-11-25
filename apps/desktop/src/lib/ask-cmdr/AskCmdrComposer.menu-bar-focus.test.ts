/**
 * The composer takes focus as it mounts and on each `requestComposerFocus()`, and ONLY then. It
 * mounts beside the in-window menu bar (Linux, Windows), whose `focusin` handler reads the bar's
 * `active` state, so a tracked `.focus()` in the composer's effect would make `active` a
 * dependency and steal focus every time the bar takes or gives back the keyboard (#1464).
 */

import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'

vi.mock('#lib/ui/toast/index.js', () => ({ addToast: vi.fn() }))
vi.mock('#lib/app-status-store.js', () => ({ saveAppStatus: vi.fn() }))
vi.mock('#lib/logging/logger.js', () => ({
  getAppLogger: () => ({ debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() }),
}))
vi.mock('./rail-window', () => ({
  growMainWindowForRail: vi.fn(() => Promise.resolve()),
  shrinkMainWindowForRail: vi.fn(() => Promise.resolve()),
}))

import AskCmdrComposer from './AskCmdrComposer.svelte'
import MenuBar from '#lib/ui/MenuBar.svelte'
import { withConventions } from '#lib/platform/test-platform.js'

const teardowns: (() => void)[] = []

afterEach(() => {
  for (const teardown of teardowns.splice(0)) teardown()
  document.body.replaceChildren()
})

/** A fresh mount target in the page, unmounted after the test. */
function target(): HTMLDivElement {
  const el = document.createElement('div')
  document.body.append(el)
  return el
}

function unmountAfter(mounted: Record<string, unknown>): void {
  teardowns.push(() => {
    void unmount(mounted)
  })
}

describe('the composer beside the in-window menu bar', () => {
  // Regression anchor (#1464): with the rail open, F2 then a click on a bar title (or Alt) re-ran
  // the composer's focus effect, which moved focus into the composer and ended the inline rename.
  it('leaves focus in a text field when the bar takes the keyboard from it, and gives it back', async () => {
    teardowns.push(withConventions('windows'))
    const bar = mount(MenuBar, {
      target: target(),
      props: {
        menus: [
          {
            id: 'menu_edit',
            title: { text: 'Edit' },
            rows: [
              {
                kind: 'item',
                id: 'edit_cut',
                label: { text: 'Cut' },
                shortcut: null,
                ariaKeyshortcuts: null,
                enabled: true,
              },
            ],
          },
        ],
        onPick: () => {},
        ariaLabel: 'Menu bar',
        flavor: 'desktop',
      },
    })
    unmountAfter(bar)
    flushSync()
    const composer = mount(AskCmdrComposer, {
      target: target(),
      props: { readSelection: () => Promise.resolve({ kind: 'refs' as const, refs: [], leftOut: 0 }) },
    })
    unmountAfter(composer)
    flushSync()
    expect(document.activeElement?.tagName).toBe('TEXTAREA')

    // The inline rename's field, say.
    const field = document.createElement('input')
    document.body.append(field)
    field.focus()

    const title = document.querySelector<HTMLElement>('.menu-bar-title')
    title?.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 }))
    flushSync()
    for (let i = 0; i < 4; i++) await tick()
    expect(document.querySelector('[data-command-menu="bar"]')).not.toBeNull()
    expect(document.activeElement).toBe(field)

    // The bar lets go (a click on the open title), and focus still stays put.
    title?.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 }))
    flushSync()
    for (let i = 0; i < 4; i++) await tick()
    expect(document.querySelector('[data-command-menu="bar"]')).toBeNull()
    expect(document.activeElement).toBe(field)
  })
})
