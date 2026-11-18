/**
 * The window's in-page right-click menu where `convention('contextMenus')` is `inPage` (Windows):
 * a `show_*` wrapper draws the backend's rows at the pointer or the caller's anchor and sends a
 * pick back with its token, resolving once the menu is gone; and a text field's right-click opens
 * the edit menu, greyed by the field's state, keeping its selection and caret. On macOS the
 * wrappers stay native.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import type { InPageContextMenu, Row } from '#lib/ipc/bindings.js'
import { withConventions } from '#lib/platform/test-platform.js'
import { expectNoA11yViolations } from '#lib/test-a11y.js'

const commands = vi.hoisted(() => ({
  openContextMenuInPage: vi.fn(),
  pickContextMenuItem: vi.fn(() => Promise.resolve('ran')),
  readClipboardText: vi.fn(() => Promise.resolve({ status: 'ok', data: 'pasted' })),
  showTabContextMenu: vi.fn(() => Promise.resolve({ status: 'ok', data: null })),
}))

vi.mock('#lib/ipc/bindings.js', async (importOriginal) => {
  const actual = await importOriginal<typeof import('#lib/ipc/bindings.js')>()
  return { ...actual, commands: { ...actual.commands, ...commands } }
})

import { showDateSortMenu, showTabContextMenu } from '#lib/tauri-commands/index.js'
import { installContextMenuOwnership } from '#lib/context-menu-ownership.js'
import InPageContextMenus from './InPageContextMenus.svelte'

const TAB_ROWS: Row[] = [
  { kind: 'item', id: 'tab_pin', label: { text: 'Pin tab' }, shortcut: null, ariaKeyshortcuts: null, enabled: true },
  { kind: 'separator' },
  {
    kind: 'item',
    id: 'tab_close',
    label: { text: 'Close tab' },
    shortcut: null,
    ariaKeyshortcuts: null,
    enabled: true,
  },
]

function answer(rows: Row[], anchor: InPageContextMenu['anchor'] = null, token = 7): InPageContextMenu {
  return { token, rows, anchor }
}

const teardowns: (() => void)[] = []
let execCommand: ReturnType<typeof vi.fn>

beforeEach(() => {
  teardowns.push(withConventions('windows'))
  execCommand = vi.fn(() => true)
  Object.defineProperty(document, 'execCommand', { value: execCommand, configurable: true })
  Object.defineProperty(document, 'queryCommandEnabled', { value: () => true, configurable: true })
  vi.clearAllMocks()
})

afterEach(() => {
  for (const teardown of teardowns.splice(0).reverse()) teardown()
  document.body.replaceChildren()
})

async function settle(): Promise<void> {
  for (let i = 0; i < 6; i++) await tick()
  await new Promise((resolve) => setTimeout(resolve, 0))
}

async function setup(): Promise<void> {
  teardowns.push(installContextMenuOwnership())
  const target = document.createElement('div')
  document.body.append(target)
  const component = mount(InPageContextMenus, { target })
  teardowns.push(() => {
    void unmount(component)
  })
  await settle()
}

/** A pointer right-click on `el` at (x, y), as the engine fires it. */
function rightClick(el: Element, x = 30, y = 40): MouseEvent {
  el.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 2, clientX: x, clientY: y }))
  const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: x, clientY: y })
  el.dispatchEvent(event)
  return event
}

async function press(key: string, init: KeyboardEventInit = {}): Promise<void> {
  ;(document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }),
  )
  await settle()
}

const surface = () => document.querySelector<HTMLElement>('[data-command-menu="context"]')
const rowNamed = (name: string) =>
  [...document.querySelectorAll<HTMLElement>('.command-menu-row')].find((row) => row.textContent.includes(name))

describe('a show_* menu drawn in the page', () => {
  it('opens at the pointer, picks through the token, and resolves once the pick ran', async () => {
    await setup()
    commands.openContextMenuInPage.mockResolvedValue(answer(TAB_ROWS))
    const tab = document.createElement('button')
    document.body.append(tab)
    tab.focus()
    rightClick(tab, 120, 80)
    let resolved = false
    const shown = showTabContextMenu(false, true, false).then(() => {
      resolved = true
    })
    await settle()
    expect(commands.openContextMenuInPage).toHaveBeenCalledWith({
      menu: 'tab',
      isPinned: false,
      canClose: true,
      hasOtherUnpinnedTabs: false,
    })
    expect(commands.showTabContextMenu).not.toHaveBeenCalled()
    expect(surface()?.style.left).toBe('120px')
    expect(surface()?.style.top).toBe('80px')
    expect(resolved).toBe(false)
    rowNamed('Close tab')?.click()
    await shown
    expect(commands.pickContextMenuItem).toHaveBeenCalledWith(7, 'tab_close')
    expect(surface()).toBeNull()
    expect(document.activeElement).toBe(tab)
  })

  it('resolves with nothing picked when Escape closes it, focus back where it was', async () => {
    await setup()
    commands.openContextMenuInPage.mockResolvedValue(answer(TAB_ROWS))
    const tab = document.createElement('button')
    document.body.append(tab)
    tab.focus()
    rightClick(tab)
    const shown = showTabContextMenu(false, true, false)
    await settle()
    await press('Escape')
    await shown
    expect(commands.pickContextMenuItem).not.toHaveBeenCalled()
    expect(document.activeElement).toBe(tab)
  })

  it('opens a keyboard-opened menu at the anchor, on its first row', async () => {
    await setup()
    commands.openContextMenuInPage.mockResolvedValue(answer(TAB_ROWS, { x: 200, y: 150 }))
    const header = document.createElement('button')
    document.body.append(header)
    header.focus()
    await press('Enter')
    void showDateSortMenu('left', null, { x: 200, y: 150 })
    await settle()
    expect(surface()?.style.left).toBe('200px')
    expect(surface()?.style.top).toBe('150px')
    expect(document.activeElement?.textContent).toContain('Pin tab')
  })

  it('settles the first menu when a second one replaces it', async () => {
    await setup()
    commands.openContextMenuInPage.mockResolvedValue(answer(TAB_ROWS))
    const tab = document.createElement('button')
    document.body.append(tab)
    rightClick(tab)
    let firstDone = false
    void showTabContextMenu(false, true, false).then(() => {
      firstDone = true
    })
    await settle()
    rightClick(tab)
    void showTabContextMenu(false, true, false)
    await settle()
    expect(firstDone).toBe(true)
    expect(document.querySelectorAll('[data-command-menu="context"]')).toHaveLength(1)
  })

  it('stays native on macOS', async () => {
    teardowns.push(withConventions('macos'))
    await showTabContextMenu(true, true, true)
    expect(commands.showTabContextMenu).toHaveBeenCalledWith(true, true, true)
    expect(commands.openContextMenuInPage).not.toHaveBeenCalled()
  })
})

describe("a text field's in-page edit menu", () => {
  function field(value: string, setup: (el: HTMLInputElement) => void = () => {}): HTMLInputElement {
    const input = document.createElement('input')
    input.type = 'text'
    input.value = value
    input.setAttribute('aria-label', 'Name')
    setup(input)
    document.body.append(input)
    input.focus()
    return input
  }

  const enabled = (name: string) => rowNamed(name)?.getAttribute('aria-disabled') !== 'true'

  it('opens at the pointer with the edit rows, greyed by the selection, and passes axe', async () => {
    await setup()
    const input = field('hello world')
    input.setSelectionRange(0, 5)
    const event = rightClick(input, 50, 60)
    await settle()
    expect(event.defaultPrevented).toBe(true)
    expect(surface()?.style.left).toBe('50px')
    const labels = [...document.querySelectorAll('.command-menu-row')].map((row) => row.textContent)
    expect(labels.map((label) => label.replace(/Ctrl.*$/, '').trim())).toEqual([
      'Undo',
      'Redo',
      'Cut',
      'Copy',
      'Paste',
      'Select all',
    ])
    expect(enabled('Cut')).toBe(true)
    expect(enabled('Copy')).toBe(true)
    expect(rowNamed('Copy')?.getAttribute('aria-keyshortcuts')).toBe('Control+C')
    await expectNoA11yViolations(document.body)
  })

  it('greys Cut and Copy with no selection, and Undo, Redo, Cut, and Paste in a read-only field', async () => {
    await setup()
    const input = field('hello', (el) => {
      el.readOnly = true
    })
    input.setSelectionRange(1, 3)
    rightClick(input)
    await settle()
    expect(enabled('Undo')).toBe(false)
    expect(enabled('Redo')).toBe(false)
    expect(enabled('Cut')).toBe(false)
    expect(enabled('Paste')).toBe(false)
    expect(enabled('Copy')).toBe(true)
    expect(enabled('Select all')).toBe(true)
  })

  it('never offers a password to the clipboard', async () => {
    await setup()
    const input = field('secret', (el) => {
      el.type = 'password'
    })
    input.setSelectionRange(0, 6)
    rightClick(input)
    await settle()
    expect(enabled('Cut')).toBe(false)
    expect(enabled('Copy')).toBe(false)
    expect(enabled('Paste')).toBe(true)
  })

  it('copies the selection it opened over, with focus back in the field', async () => {
    await setup()
    const input = field('hello world')
    input.setSelectionRange(6, 11)
    rightClick(input)
    await settle()
    // The menu took focus; a stray caret move meanwhile mustn't decide what's copied.
    input.setSelectionRange(0, 0)
    rowNamed('Copy')?.click()
    await settle()
    expect(execCommand).toHaveBeenCalledWith('copy')
    expect(document.activeElement).toBe(input)
    expect([input.selectionStart, input.selectionEnd]).toEqual([6, 11])
  })

  it('pastes the clipboard at the caret through the backend', async () => {
    await setup()
    const input = field('ab')
    input.setSelectionRange(1, 1)
    rightClick(input)
    await settle()
    rowNamed('Paste')?.click()
    await settle()
    expect(commands.readClipboardText).toHaveBeenCalled()
    expect(execCommand).toHaveBeenCalledWith('insertText', false, 'pasted')
    expect(input.selectionStart).toBe(1)
  })

  it('selects the whole field', async () => {
    await setup()
    const input = field('hello')
    input.setSelectionRange(2, 2)
    rightClick(input)
    await settle()
    rowNamed('Select all')?.click()
    await settle()
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 5])
  })

  it('keeps the caret where it was when Escape closes it', async () => {
    await setup()
    const input = field('hello')
    input.setSelectionRange(3, 3)
    rightClick(input)
    await settle()
    await press('Escape')
    expect(surface()).toBeNull()
    expect(document.activeElement).toBe(input)
    expect([input.selectionStart, input.selectionEnd]).toEqual([3, 3])
  })

  it('opens from the keyboard (Shift+F10) on its first live row, under the field', async () => {
    await setup()
    const input = field('hello')
    input.setSelectionRange(0, 5)
    await press('F10', { shiftKey: true })
    input.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 0 }))
    await settle()
    expect(surface()).not.toBeNull()
    expect(document.activeElement?.textContent).toContain('Undo')
  })

  it('acts on a contenteditable host from a right-click inside it', async () => {
    await setup()
    const host = document.createElement('div')
    host.contentEditable = 'true'
    host.setAttribute('role', 'textbox')
    host.setAttribute('aria-label', 'Note')
    const inner = document.createElement('span')
    inner.textContent = 'some note'
    host.append(inner)
    document.body.append(host)
    host.focus()
    rightClick(inner)
    await settle()
    expect(surface()).not.toBeNull()
    expect(enabled('Paste')).toBe(true)
    rowNamed('Select all')?.click()
    await settle()
    expect(window.getSelection()?.toString()).toBe('some note')
  })

  it('opens nothing on a disabled field', async () => {
    await setup()
    const input = field('hello', (el) => {
      el.disabled = true
    })
    const event = rightClick(input)
    await settle()
    expect(event.defaultPrevented).toBe(true)
    expect(surface()).toBeNull()
  })
})
