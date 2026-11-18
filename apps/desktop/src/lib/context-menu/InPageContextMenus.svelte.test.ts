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
import InlineRenameEditor from '#lib/file-explorer/rename/InlineRenameEditor.svelte'
import Menu from '#lib/ui/Menu.svelte'
import { createMenu } from '#lib/ui/menu-controller.svelte.js'

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
  el.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 2, clientX: x, clientY: y }))
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

/**
 * A left click on `el` the way Chromium delivers it: a `preventDefault`ed `pointerdown` suppresses
 * the compatibility `mousedown` (and with it the focus move), but never the `click`. jsdom models
 * neither, so a bare `.click()` would skip what a real click sends.
 */
async function clickLikeABrowser(el: Element | undefined): Promise<void> {
  if (!el) throw new Error('nothing to click')
  const down = new MouseEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 })
  el.dispatchEvent(down)
  if (!down.defaultPrevented) {
    el.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0 }))
    if (el instanceof HTMLElement && el.tabIndex >= -1) el.focus()
  }
  el.dispatchEvent(new MouseEvent('pointerup', { bubbles: true, button: 0 }))
  if (!down.defaultPrevented) el.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, button: 0 }))
  el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 }))
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
    await clickLikeABrowser(rowNamed('Close tab'))
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
    await clickLikeABrowser(rowNamed('Copy'))
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
    await clickLikeABrowser(rowNamed('Paste'))
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
    await clickLikeABrowser(rowNamed('Select all'))
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
    // Focus stays in the field; the cursor is its active descendant.
    expect(document.activeElement).toBe(input)
    const active = document.getElementById(input.getAttribute('aria-activedescendant') ?? '')
    expect(active?.textContent).toContain('Undo')
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
    await clickLikeABrowser(rowNamed('Select all'))
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

  // Regression anchor (review of #1264): the menu took focus from the field, so the inline rename
  // editor's blur cancelled the rename and Paste found a detached input.
  it('pastes into the inline rename editor without ending the rename', async () => {
    await setup()
    const handlers = {
      onInput: vi.fn(),
      onSubmit: vi.fn(),
      onCancel: vi.fn(),
      onClickAway: vi.fn(),
      onShakeEnd: vi.fn(),
    }
    const target = document.createElement('div')
    document.body.append(target)
    const editor = mount(InlineRenameEditor, {
      target,
      props: {
        value: 'report.md',
        severity: 'ok',
        shaking: false,
        ariaLabel: 'Rename report.md',
        sessionId: 1,
        ...handlers,
      },
    })
    teardowns.push(() => {
      void unmount(editor)
    })
    await settle()
    const input = target.querySelector<HTMLInputElement>('.rename-input')
    if (!input) throw new Error('no rename field')
    expect(document.activeElement).toBe(input)
    input.setSelectionRange(6, 6)
    rightClick(input)
    await settle()
    expect(surface()).not.toBeNull()
    expect(document.activeElement).toBe(input)
    await clickLikeABrowser(rowNamed('Paste'))
    expect(handlers.onCancel).not.toHaveBeenCalled()
    expect(handlers.onClickAway).not.toHaveBeenCalled()
    expect(document.activeElement).toBe(input)
    expect(execCommand).toHaveBeenCalledWith('insertText', false, 'pasted')
  })

  // Regression anchor (review of #1264): a pointer-down on a context menu row closed the popup menu
  // the field sits in (the volume switcher's favorite rename), unmounting the field.
  it('pastes into a field inside an open popup menu without closing it', async () => {
    await setup()
    const popup = createMenu({
      getSections: () => [{ id: 'favorites', items: [{ value: 'projects', label: 'Projects' }] }],
      onSelect: () => {},
    })
    popup.openAt({ x: 10, y: 10 })
    const target = document.createElement('div')
    document.body.append(target)
    const component = mount(Menu, { target, props: { menu: popup, ariaLabel: 'Volumes' } })
    teardowns.push(() => {
      popup.destroy()
      void unmount(component)
    })
    await settle()
    const rename = document.createElement('input')
    rename.setAttribute('aria-label', 'Favorite name')
    rename.value = 'Projects'
    document.querySelector('[data-menu]')?.append(rename)
    rename.focus()
    rightClick(rename)
    await settle()
    await clickLikeABrowser(rowNamed('Paste'))
    expect(popup.isOpen).toBe(true)
    expect(rename.isConnected).toBe(true)
    expect(execCommand).toHaveBeenCalledWith('insertText', false, 'pasted')
  })

  it('keeps the menu-holding field focused while the keyboard walks the menu, typing nothing into it', async () => {
    await setup()
    const input = field('hello')
    const blurred = vi.fn()
    input.addEventListener('blur', blurred)
    await press('F10', { shiftKey: true })
    input.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 0 }))
    await settle()
    const first = document.querySelector('.command-menu-row')
    expect(input.getAttribute('aria-activedescendant')).toBe(first?.id)
    const typed = new KeyboardEvent('keydown', { key: 'q', bubbles: true, cancelable: true })
    input.dispatchEvent(typed)
    expect(typed.defaultPrevented).toBe(true)
    await press('Escape')
    expect(blurred).not.toHaveBeenCalled()
    expect(input.hasAttribute('aria-activedescendant')).toBe(false)
  })
})

/** Edges from the review of #1264: late answers, a refused or moved paste, an input method, greyed Undo, a recycled row. */
describe('the in-page menu at its edges', () => {
  function labelled(value: string): HTMLInputElement {
    const input = document.createElement('input')
    input.type = 'text'
    input.value = value
    input.setAttribute('aria-label', 'Name')
    document.body.append(input)
    input.focus()
    return input
  }

  it('drops an answer that lands after a later right-click asked', async () => {
    await setup()
    let answerFirst: (menu: InPageContextMenu) => void = () => {}
    commands.openContextMenuInPage
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            answerFirst = resolve
          }),
      )
      .mockResolvedValueOnce(answer(TAB_ROWS, null, 2))
    const tab = document.createElement('button')
    document.body.append(tab)
    rightClick(tab)
    const first = showTabContextMenu(false, true, false)
    rightClick(tab)
    void showTabContextMenu(false, true, false)
    await settle()
    answerFirst(answer(TAB_ROWS, null, 1))
    await first
    await settle()
    expect(document.querySelectorAll('[data-command-menu="context"]')).toHaveLength(1)
    await clickLikeABrowser(rowNamed('Pin tab'))
    expect(commands.pickContextMenuItem).toHaveBeenCalledWith(2, 'tab_pin')
  })

  it('drops an answer asked before a text field opened its own menu', async () => {
    await setup()
    let late: (menu: InPageContextMenu) => void = () => {}
    commands.openContextMenuInPage.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          late = resolve
        }),
    )
    const shown = showTabContextMenu(false, true, false)
    const input = labelled('hello')
    rightClick(input)
    await settle()
    late(answer(TAB_ROWS))
    await shown
    await settle()
    expect(rowNamed('Pin tab')).toBeUndefined()
    expect(rowNamed('Paste')).toBeDefined()
  })

  it('pastes nothing into a field that refuses a paste', async () => {
    await setup()
    const input = labelled('ab')
    input.addEventListener('paste', (event) => {
      event.preventDefault()
    })
    rightClick(input)
    await settle()
    await clickLikeABrowser(rowNamed('Paste'))
    expect(execCommand).not.toHaveBeenCalledWith('insertText', false, 'pasted')
  })

  it("hands the field its paste with the clipboard's text, as a real paste does", async () => {
    await setup()
    const input = labelled('ab')
    let offered = ''
    input.addEventListener('paste', (event) => {
      offered = event.clipboardData?.getData('text/plain') ?? ''
    })
    rightClick(input)
    await settle()
    await clickLikeABrowser(rowNamed('Paste'))
    expect(offered).toBe('pasted')
    expect(execCommand).toHaveBeenCalledWith('insertText', false, 'pasted')
  })

  it('pastes nothing when focus left the field while the clipboard was read', async () => {
    await setup()
    const input = labelled('ab')
    const elsewhere = labelled('other')
    input.focus()
    let read: (value: { status: 'ok'; data: string }) => void = () => {}
    commands.readClipboardText.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          read = resolve
        }),
    )
    rightClick(input)
    await settle()
    await clickLikeABrowser(rowNamed('Paste'))
    elsewhere.focus()
    read({ status: 'ok', data: 'pasted' })
    await settle()
    expect(execCommand).not.toHaveBeenCalledWith('insertText', false, 'pasted')
  })

  it('opens no menu while an input method is composing, and closes one a composition starts under', async () => {
    await setup()
    const input = labelled('hello')
    input.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rightClick(input)
    await settle()
    expect(surface()).toBeNull()
    input.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    rightClick(input)
    await settle()
    expect(surface()).not.toBeNull()
    input.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    await settle()
    expect(surface()).toBeNull()
  })

  it('greys Undo and Redo when the field has nothing to undo or redo', async () => {
    Object.defineProperty(document, 'queryCommandEnabled', { value: () => false, configurable: true })
    await setup()
    const input = labelled('hello')
    rightClick(input)
    await settle()
    expect(rowNamed('Undo')?.getAttribute('aria-disabled')).toBe('true')
    expect(rowNamed('Redo')?.getAttribute('aria-disabled')).toBe('true')
  })

  it('still closes on a scroll of the list after the row it opened on was re-rendered', async () => {
    await setup()
    commands.openContextMenuInPage.mockResolvedValue(answer(TAB_ROWS))
    const list = document.createElement('div')
    const row = document.createElement('div')
    list.append(row)
    document.body.append(list)
    rightClick(row)
    void showTabContextMenu(false, true, false)
    await settle()
    expect(surface()).not.toBeNull()
    row.remove()
    list.append(document.createElement('div'))
    list.dispatchEvent(new Event('scroll'))
    await settle()
    expect(surface()).toBeNull()
  })
})
