import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { withConventions, withPlatform } from '#lib/platform/test-platform.js'
import { tick } from 'svelte'
import { createContextMenuHost, type ContextMenuHost } from './context-menu.js'
import type { ContextPopup, MenuClick } from './menu-engine.js'

// A fake popup with the engine's shapes (`menu-engine.ts`, `cmdr-menu`'s page rows), so the test
// pins what this file does with them: draw on the shared component, walk, and send what a pick asks for.
const POPUP: ContextPopup = {
  anchor: null,
  rows: [
    { kind: 'header', label: { text: 'report.pdf · 2.1 MB' } },
    { kind: 'separator' },
    {
      kind: 'item',
      id: 'file_view',
      label: { key: 'menu.file.view', args: {} },
      shortcut: 'F3',
      ariaKeyshortcuts: 'F3',
      enabled: true,
    },
    {
      kind: 'item',
      id: 'rename',
      label: { key: 'menu.file.rename', args: {} },
      shortcut: null,
      ariaKeyshortcuts: null,
      enabled: false,
    },
    {
      kind: 'submenu',
      id: 'menu.context.selection',
      label: { key: 'menu.context.selection', args: {} },
      enabled: true,
      rows: [
        {
          kind: 'item',
          id: 'toggle_selection',
          label: { key: 'menu.context.toggleSelection', args: {} },
          shortcut: 'Space',
          ariaKeyshortcuts: 'Space',
          enabled: true,
        },
      ],
    },
    {
      kind: 'item',
      id: 'file_copy',
      label: { key: 'menu.file.copy', args: {} },
      shortcut: 'F5',
      ariaKeyshortcuts: 'F5',
      enabled: true,
    },
  ],
}

let host: ContextMenuHost | undefined

function setup(
  contextClick: (id: string) => MenuClick = (id) => ({
    kind: 'emit',
    event: 'execute-command',
    payload: { commandId: id },
    to: 'main',
  }),
) {
  const emitted: [string, unknown, unknown][] = []
  const picked: string[] = []
  const invoke = vi.fn(() => Promise.resolve(null))
  host = createContextMenuHost({
    menu: {
      contextClick: (id) => {
        picked.push(id)
        return contextClick(id)
      },
    },
    emit: (event, payload, target) => emitted.push([event, payload, target]),
    invoke,
    windows: { front: vi.fn(() => undefined), close: vi.fn() },
  })
  return { emitted, picked, invoke }
}

async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) await tick()
}

async function open(fromKeyboard: boolean, at = { x: 0, y: 0 }): Promise<void> {
  host?.open(POPUP, at, fromKeyboard)
  await settle()
}

async function key(name: string): Promise<KeyboardEvent> {
  const event = new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true })
  ;(document.activeElement ?? document.body).dispatchEvent(event)
  await settle()
  return event
}

/** Plays a visitor on `os`: its platform and its desktop's conventions. Returns the undo. */
function visitor(os: 'macos' | 'windows'): () => void {
  const undoPlatform = withPlatform(os)
  const undoConventions = withConventions(os)
  return () => {
    undoConventions()
    undoPlatform()
  }
}

const menus = () => [...document.querySelectorAll<HTMLElement>('[data-command-menu="context"]')]
const rows = () => [...document.querySelectorAll<HTMLElement>('.command-menu-row')]

afterEach(() => {
  host?.destroy()
  host = undefined
  document.body.replaceChildren()
})

describe('demo context menu', () => {
  // A Mac visitor: macOS 27's menus.
  let undoVisitor: (() => void) | undefined
  beforeEach(() => {
    undoVisitor = visitor('macos')
  })
  afterEach(() => {
    undoVisitor?.()
  })

  it('draws the rows as a macOS menu named by its header, at the point it was asked for', async () => {
    setup()
    await open(false, { x: 40, y: 60 })
    const [menu] = menus()
    expect(menu.getAttribute('role')).toBe('menu')
    expect(menu.classList.contains('cmdr-demo-macos-menu')).toBe(true)
    expect(menu.getAttribute('aria-label')).toBe('report.pdf · 2.1 MB')
    expect(menu.style.left).toBe('40px')
    expect(menu.style.top).toBe('60px')
    const header = menu.querySelector('.command-menu-header')
    expect(header?.textContent).toBe('report.pdf · 2.1 MB')
    expect(header?.getAttribute('aria-hidden')).toBe('true')
    expect(rows().map((row) => row.getAttribute('role'))).toEqual(['menuitem', 'menuitem', 'menuitem', 'menuitem'])
    expect(rows()[1].getAttribute('aria-disabled')).toBe('true')
    expect(rows()[0].getAttribute('aria-keyshortcuts')).toBe('F3')
  })

  it('starts on the first enabled row from the keyboard, skips greyed rows, and picks with Enter', async () => {
    const { emitted, picked } = setup()
    const trigger = document.createElement('button')
    document.body.append(trigger)
    trigger.focus()
    await open(true)
    expect(document.activeElement?.textContent).toContain('View')
    await key('ArrowDown')
    // Rename is greyed: the walk lands on the submenu row.
    expect(document.activeElement?.getAttribute('aria-haspopup')).toBe('menu')
    await key('ArrowDown')
    const enter = await key('Enter')
    expect(enter.defaultPrevented).toBe(true)
    expect(picked).toEqual(['file_copy'])
    expect(emitted).toEqual([['execute-command', { commandId: 'file_copy' }, { kind: 'label', label: 'main' }]])
    expect(menus()).toHaveLength(0)
    // Focus goes back where it was, as closing a native menu does.
    expect(document.activeElement).toBe(trigger)
  })

  it('opens a submenu with → and backs out with ← and Esc', async () => {
    setup()
    await open(true)
    await key('ArrowDown')
    await key('ArrowRight')
    expect(menus()).toHaveLength(2)
    expect(document.activeElement?.textContent).toContain('Space')
    await key('ArrowLeft')
    expect(menus()).toHaveLength(1)
    await key('ArrowRight')
    await key('Escape')
    expect(menus()).toHaveLength(1)
    await key('Escape')
    expect(menus()).toHaveLength(0)
  })

  it('keeps every key from the app while it is up, and picks no row by a letter (a Mac menu has no access keys)', async () => {
    const { picked } = setup()
    const appKeys = vi.fn()
    window.addEventListener('keydown', appKeys)
    await open(true)
    await key('c')
    await key('ArrowDown')
    window.removeEventListener('keydown', appKeys)
    expect(appKeys).not.toHaveBeenCalled()
    expect(picked).toEqual([])
  })

  it('never picks a greyed row, and a click outside closes it', async () => {
    const { picked } = setup()
    await open(false)
    rows()[1].click()
    expect(picked).toEqual([])
    expect(menus()).toHaveLength(1)
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
    await settle()
    expect(menus()).toHaveLength(0)
  })

  it('runs a command pick through invoke, as the desktop calls `add_favorite`', async () => {
    const { invoke } = setup(() => ({ kind: 'invoke', command: 'add_favorite', args: { path: '/a', name: null } }))
    await open(false)
    rows()[0].click()
    expect(invoke).toHaveBeenCalledWith('add_favorite', { path: '/a', name: null })
  })
})

// Task 63 step 11: on Windows the right-click menus are in-page too, in Windows 11's look.
describe('demo context menu for a Windows visitor', () => {
  let undoVisitor: (() => void) | undefined
  beforeEach(() => {
    undoVisitor = visitor('windows')
  })
  afterEach(() => {
    undoVisitor?.()
  })

  it('draws a Windows menu, not a Mac one', async () => {
    setup()
    await open(false, { x: 40, y: 60 })
    const [menu] = menus()
    expect(menu.getAttribute('role')).toBe('menu')
    expect(menu.classList.contains('cmdr-demo-macos-menu')).toBe(false)
    expect(menu.classList.contains('mac')).toBe(false)
  })
})
