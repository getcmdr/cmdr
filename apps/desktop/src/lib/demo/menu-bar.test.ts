import { flushSync, tick } from 'svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { withConventions, withPlatform } from '#lib/platform/test-platform.js'
import { _setCatalogForTests, setLocale } from '#lib/intl/messages.svelte.js'
import { rowLabelText } from '#lib/ui/command-menu/rows.js'
import { installMenuBar, type MenuBarHandle } from './menu-bar.svelte.js'
import type { BarMenu, MenuClick } from './menu-engine.js'

// A fake engine with the real one's shapes (`menu-engine.ts`, `cmdr-menu`'s page rows), so the
// test pins what this file does with them: draw on the shared component, walk, and send what a
// click asks for.
const BAR: BarMenu[] = [
  {
    id: 'menu_app',
    title: { text: 'Cmdr' },
    rows: [
      {
        kind: 'item',
        id: 'about',
        label: { key: 'menu.app.about', args: {} },
        shortcut: null,
        ariaKeyshortcuts: null,
        enabled: true,
      },
      { kind: 'separator' },
      { kind: 'system', label: { key: 'menu.app.quit', args: {} } },
    ],
  },
  {
    id: 'menu_view',
    title: { key: 'menu.bar.view', args: {} },
    rows: [
      {
        kind: 'item',
        id: 'file_copy',
        label: { key: 'menu.file.copy', args: {} },
        shortcut: 'F5',
        ariaKeyshortcuts: 'F5',
        enabled: false,
      },
      {
        kind: 'check',
        id: 'show_hidden_files',
        label: { key: 'menu.view.showHiddenFiles', args: {} },
        shortcut: '⇧⌘.',
        ariaKeyshortcuts: 'Shift+Meta+.',
        enabled: true,
        checked: true,
        checkKind: 'checkbox',
      },
      {
        kind: 'submenu',
        id: 'menu.view.sortBy',
        label: { key: 'menu.view.sortBy', args: {} },
        enabled: true,
        rows: [
          {
            kind: 'check',
            id: 'sort_by_name',
            label: { key: 'menu.sort.name', args: {} },
            shortcut: '⌃3',
            ariaKeyshortcuts: 'Control+3',
            enabled: true,
            checked: false,
            checkKind: 'radio',
          },
        ],
      },
      {
        kind: 'item',
        id: 'go_home',
        label: { key: 'menu.go.home', args: {} },
        shortcut: '⇧⌘H',
        ariaKeyshortcuts: 'Shift+Meta+H',
        enabled: true,
      },
    ],
  },
]

let handle: MenuBarHandle | undefined

function setup(
  click: (id: string) => MenuClick = (id) => ({
    kind: 'emit',
    event: 'execute-command',
    payload: { commandId: id },
    to: 'main',
  }),
) {
  const emitted: [string, unknown, unknown][] = []
  const clicked: string[] = []
  const windows = { front: vi.fn(() => undefined), close: vi.fn() }
  const listeners = new Set<() => void>()
  handle = installMenuBar({
    menu: {
      ready: Promise.resolve(true),
      bar: () => BAR,
      click: (id) => {
        clicked.push(id)
        return click(id)
      },
      onChange: (listener) => {
        listeners.add(listener)
        return () => listeners.delete(listener)
      },
    },
    emit: (event, payload, target) => emitted.push([event, payload, target]),
    invoke: vi.fn(() => Promise.resolve(null)),
    windows,
  })
  return { emitted, clicked, windows }
}

async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) await tick()
}

async function key(name: string, init: KeyboardEventInit = {}): Promise<KeyboardEvent> {
  const event = new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init })
  ;(document.activeElement ?? document.body).dispatchEvent(event)
  await settle()
  return event
}

const titles = () => [...document.querySelectorAll<HTMLElement>('.menu-bar-title')]
const rows = () => [...document.querySelectorAll<HTMLElement>('.command-menu-row')]

async function openTitle(index: number): Promise<void> {
  titles()[index].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
  await settle()
}

afterEach(() => {
  handle?.destroy()
  handle = undefined
  document.body.replaceChildren()
})

describe('demo menu bar', () => {
  // A Mac visitor: the macOS bar, its look, and its keys.
  let undoVisitor: (() => void) | undefined
  beforeEach(() => {
    const undoPlatform = withPlatform('macos')
    const undoConventions = withConventions('macos')
    undoVisitor = () => {
      undoConventions()
      undoPlatform()
    }
  })
  afterEach(() => {
    undoVisitor?.()
  })

  it('draws the titles in a menubar at the top of the page, the app menu first and bold', async () => {
    setup()
    await settle()
    const bar = document.querySelector('[role="menubar"]')
    expect(bar?.classList.contains('cmdr-demo-macos-menu')).toBe(true)
    expect(document.body.firstElementChild?.contains(bar ?? null)).toBe(true)
    expect(titles().map((t) => t.textContent.trim())).toEqual(['Cmdr', 'View'])
    expect(titles()[0].matches(':first-child')).toBe(true)
    expect(titles().every((t) => t.getAttribute('aria-haspopup') === 'menu')).toBe(true)
    // A Mac bar has no access keys.
    expect(titles().every((t) => !t.hasAttribute('aria-keyshortcuts'))).toBe(true)
  })

  it('opens a menu on click with its rows, checks, shortcuts, and greyed rows', async () => {
    setup()
    await settle()
    await openTitle(1)
    expect(titles()[1].getAttribute('aria-expanded')).toBe('true')
    const [copy, hidden, sortBy, home] = rows()
    expect(copy.getAttribute('aria-disabled')).toBe('true')
    expect(hidden.getAttribute('role')).toBe('menuitemcheckbox')
    expect(hidden.getAttribute('aria-checked')).toBe('true')
    expect(hidden.querySelector('.command-menu-shortcut')?.textContent.replace(/\s/g, '')).toBe('⇧⌘.')
    expect(hidden.querySelectorAll('.command-menu-shortcut-glyph')).toHaveLength(3)
    expect(sortBy.getAttribute('aria-haspopup')).toBe('menu')
    expect(home.textContent).toContain('Home')
  })

  it('draws an OS row as a greyed one, since a page has nothing behind it', async () => {
    setup()
    await settle()
    await openTitle(0)
    expect(rows()[1].getAttribute('aria-disabled')).toBe('true')
  })

  it("sends a picked row's event to the main window, then closes", async () => {
    const { emitted, clicked } = setup()
    await settle()
    await openTitle(1)
    rows()[3].click()
    await settle()
    expect(clicked).toEqual(['go_home'])
    expect(emitted).toEqual([['execute-command', { commandId: 'go_home' }, { kind: 'label', label: 'main' }]])
    expect(rows()).toHaveLength(0)
  })

  it('never picks a greyed row', async () => {
    const { clicked } = setup()
    await settle()
    await openTitle(1)
    rows()[0].click()
    expect(clicked).toEqual([])
  })

  it('walks the keyboard the way a Mac menu does, and keeps the keys from the app', async () => {
    const { clicked } = setup()
    await settle()
    const appKeys: string[] = []
    const listener = (event: KeyboardEvent) => appKeys.push(event.key)
    document.addEventListener('keydown', listener)
    try {
      // ⌃F2 focuses the bar; → moves to View; ↓ opens it on its first ENABLED row.
      await key('F2', { ctrlKey: true })
      expect(document.activeElement).toBe(titles()[0])
      await key('ArrowRight')
      expect(document.activeElement).toBe(titles()[1])
      await key('ArrowDown')
      expect(document.activeElement?.textContent).toContain('Show hidden files')
      // ↓ to the submenu row, → opens it on its first row, ← closes it.
      await key('ArrowDown')
      await key('ArrowRight')
      expect(document.activeElement?.textContent).toContain('Name')
      await key('ArrowLeft')
      expect(document.activeElement?.textContent).toContain('Sort by')
      await key('ArrowDown')
      await key('Enter')
      expect(clicked).toEqual(['go_home'])
      expect(appKeys).toEqual([])
    } finally {
      document.removeEventListener('keydown', listener)
    }
  })

  it('backs out one level per Escape, then gives focus back', async () => {
    setup()
    await settle()
    const pane = document.createElement('button')
    document.body.append(pane)
    pane.focus()
    await key('F2', { ctrlKey: true })
    await key('ArrowDown')
    expect(rows().length).toBeGreaterThan(0)
    await key('Escape')
    expect(rows()).toHaveLength(0)
    expect(document.activeElement).toBe(titles()[0])
    await key('Escape')
    expect(document.activeElement).toBe(pane)
  })

  it('closes the window in front for Close tab while another window has focus', async () => {
    const frame = document.createElement('iframe')
    const { windows } = setup(() => ({ kind: 'closeFocusedWindow' }))
    windows.front.mockReturnValue({ label: 'settings', frame } as never)
    await settle()
    await openTitle(1)
    rows()[3].click()
    expect(windows.close).toHaveBeenCalledWith('settings')
  })

  it('fills a live label’s tokens literally, as the native menu does', () => {
    expect(rowLabelText({ key: 'menu.select.sameExtension', args: { extension: 'pdf' } })).toBe(
      'Select all with extension *.pdf',
    )
    expect(rowLabelText({ text: 'Cmdr' })).toBe('Cmdr')
  })

  it('redraws the closed strip in the new language at once, as the rest of the UI does', async () => {
    _setCatalogForTests('hu', { 'menu.bar.view': 'Nézet', 'ui.menuBar.label': 'Menüsor' })
    try {
      setup()
      await settle()
      expect(titles().map((t) => t.textContent.trim())).toEqual(['Cmdr', 'View'])
      await setLocale('hu')
      flushSync()
      expect(titles().map((t) => t.textContent.trim())).toEqual(['Cmdr', 'Nézet'])
      expect(document.querySelector('[role="menubar"]')?.getAttribute('aria-label')).toBe('Menüsor')
      expect(rows()).toHaveLength(0)
    } finally {
      await setLocale(null)
      _setCatalogForTests('hu', null)
    }
  })
})

// Task 63 step 11: a Windows visitor sees the Windows bar (the engine draws Windows' rows), in
// Windows 11's look and keys, with the window under it at that bar's own height.
describe('demo menu bar for a Windows visitor', () => {
  let undoVisitor: (() => void) | undefined
  beforeEach(() => {
    const undoPlatform = withPlatform('windows')
    const undoConventions = withConventions('windows')
    undoVisitor = () => {
      undoConventions()
      undoPlatform()
    }
  })
  afterEach(() => {
    undoVisitor?.()
  })

  it('wears the Windows look and keys, not the Mac’s', async () => {
    setup()
    await settle()
    const bar = document.querySelector('[role="menubar"]')
    expect(bar?.classList.contains('cmdr-demo-macos-menu')).toBe(false)
    expect(bar?.classList.contains('mac')).toBe(false)
    // Alt plus a title's letter opens it, as on Windows.
    expect(titles().every((t) => t.getAttribute('aria-keyshortcuts')?.startsWith('Alt+'))).toBe(true)
  })

  it('starts the window under the bar at the look’s own bar height', () => {
    setup()
    expect(document.documentElement.style.getPropertyValue('--cmdr-demo-window-top')).toBe(
      'var(--spacing-menu-bar-height)',
    )
  })

  it('starts the window under the main window’s title bar too, where one is drawn above the bar', () => {
    document.documentElement.style.setProperty('--cmdr-demo-title-bar-height', '32px')
    try {
      setup()
      expect(document.documentElement.style.getPropertyValue('--cmdr-demo-window-top')).toBe(
        'calc(var(--cmdr-demo-title-bar-height) + var(--spacing-menu-bar-height))',
      )
    } finally {
      document.documentElement.style.removeProperty('--cmdr-demo-title-bar-height')
    }
  })

  it('opens on a lone Alt, as a Windows bar does', async () => {
    setup()
    await settle()
    await key('Alt')
    document.dispatchEvent(new KeyboardEvent('keyup', { key: 'Alt', bubbles: true }))
    await settle()
    expect(document.activeElement).toBe(titles()[0])
  })
})
