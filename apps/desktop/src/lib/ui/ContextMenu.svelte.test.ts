/**
 * `ContextMenu` (`command-menu/context-menu.svelte.ts`): opening at a point, the keys, a pick,
 * dismissal (Esc, a click elsewhere, blur, a scroll of what it was opened on, a dialog), focus
 * back where it was, the cue rule, and placement at each window corner.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import type { Row } from '#lib/ipc/bindings.js'
import { withConventions } from '#lib/platform/test-platform.js'
import { expectNoA11yViolations } from '#lib/test-a11y.js'
import ContextMenu from './ContextMenu.svelte'
import { createContextMenu, type ContextMenuController } from './command-menu/context-menu.svelte'
import { markDialogClosed, markDialogOpen } from './open-dialogs.svelte'

const ROWS: Row[] = [
  { kind: 'header', label: { text: 'report.pdf' } },
  { kind: 'item', id: 'open', label: { text: 'Open' }, shortcut: 'Enter', ariaKeyshortcuts: 'Enter', enabled: true },
  { kind: 'item', id: 'copy', label: { text: 'Copy' }, shortcut: 'F5', ariaKeyshortcuts: 'F5', enabled: false },
  { kind: 'separator' },
  {
    kind: 'submenu',
    id: 'menu.context.openWith',
    label: { text: 'Open with' },
    enabled: true,
    rows: [
      {
        kind: 'item',
        id: 'open_with_viewer',
        label: { text: 'Viewer' },
        shortcut: null,
        ariaKeyshortcuts: null,
        enabled: true,
      },
    ],
  },
  { kind: 'item', id: 'delete', label: { text: 'Delete' }, shortcut: 'F8', ariaKeyshortcuts: 'F8', enabled: true },
]

const teardowns: (() => void)[] = []

beforeEach(() => {
  withConventions('windows')
})

afterEach(() => {
  for (const teardown of teardowns.splice(0)) teardown()
  document.body.replaceChildren()
})

async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) await tick()
}

async function setup(): Promise<{ menu: ContextMenuController; picked: string[]; pane: HTMLButtonElement }> {
  const pane = document.createElement('button')
  pane.textContent = 'report.pdf'
  document.body.append(pane)
  pane.focus()
  const picked: string[] = []
  const menu = createContextMenu({ onPick: (id) => picked.push(id) })
  const target = document.createElement('div')
  document.body.append(target)
  const component = mount(ContextMenu, { target, props: { menu } })
  teardowns.push(() => {
    void unmount(component)
  })
  await settle()
  return { menu, picked, pane }
}

async function press(key: string, init: KeyboardEventInit = {}): Promise<KeyboardEvent> {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init })
  ;(document.activeElement ?? document.body).dispatchEvent(event)
  await settle()
  return event
}

const surfaces = () => [...document.querySelectorAll<HTMLElement>('[data-command-menu="context"]')]
const rows = (depth = 0) => [...(surfaces()[depth]?.querySelectorAll<HTMLElement>('.command-menu-row') ?? [])]
const focusedText = () => document.activeElement?.textContent.trim()

describe('ContextMenu', () => {
  it('opens at the point, named by its header line, and passes axe', async () => {
    const { menu } = await setup()
    menu.open({ rows: ROWS, at: { x: 40, y: 50 }, fromKeyboard: false })
    await settle()
    const [surface] = surfaces()
    expect(surface.getAttribute('role')).toBe('menu')
    expect(surface.getAttribute('aria-label')).toBe('report.pdf')
    expect(surface.querySelector('.command-menu-header')?.getAttribute('aria-hidden')).toBe('true')
    expect(surface.style.left).toBe('40px')
    expect(surface.style.top).toBe('50px')
    // Opened by a click: no cursor yet, the surface itself holds focus.
    expect(document.activeElement).toBe(surface)
    await expectNoA11yViolations(document.body)
  })

  it('starts on the first live row when the keyboard opened it, and walks, enters, and picks', async () => {
    const { menu, picked, pane } = await setup()
    menu.open({ rows: ROWS, at: { x: 40, y: 50 }, fromKeyboard: true })
    await settle()
    expect(focusedText()).toContain('Open')
    await press('ArrowDown')
    // Copy is greyed: skipped.
    expect(focusedText()).toContain('Open with')
    await press('ArrowRight')
    expect(focusedText()).toContain('Viewer')
    await press('ArrowLeft')
    expect(surfaces()).toHaveLength(1)
    await press('End')
    expect(focusedText()).toContain('Delete')
    await press('Enter')
    expect(picked).toEqual(['delete'])
    expect(surfaces()).toHaveLength(0)
    expect(document.activeElement).toBe(pane)
  })

  it('picks a row by its letter on Windows', async () => {
    const { menu, picked } = await setup()
    menu.open({ rows: ROWS, at: { x: 40, y: 50 }, fromKeyboard: true })
    await settle()
    // Open o, Copy c, Open with w (o taken), Delete d.
    await press('d')
    expect(picked).toEqual(['delete'])
  })

  it('never picks a greyed row', async () => {
    const { menu, picked } = await setup()
    menu.open({ rows: ROWS, at: { x: 40, y: 50 }, fromKeyboard: false })
    await settle()
    rows()[1].click()
    expect(picked).toEqual([])
    expect(surfaces()).toHaveLength(1)
  })

  it('backs out with Escape one level at a time, then gives focus back', async () => {
    const { menu, pane } = await setup()
    menu.open({ rows: ROWS, at: { x: 40, y: 50 }, fromKeyboard: true })
    await settle()
    await press('ArrowDown')
    await press('ArrowRight')
    expect(surfaces()).toHaveLength(2)
    await press('Escape')
    expect(surfaces()).toHaveLength(1)
    await press('Escape')
    expect(surfaces()).toHaveLength(0)
    expect(document.activeElement).toBe(pane)
  })

  it('keeps every key from the app while it is up', async () => {
    const { menu } = await setup()
    const appKeys: string[] = []
    const listener = (event: KeyboardEvent) => appKeys.push(event.key)
    document.addEventListener('keydown', listener)
    try {
      menu.open({ rows: ROWS, at: { x: 40, y: 50 }, fromKeyboard: true })
      await settle()
      await press('ArrowDown')
      await press('F5')
      expect(appKeys).toEqual([])
    } finally {
      document.removeEventListener('keydown', listener)
    }
  })

  it('closes without moving focus on a click elsewhere, blur, resize, a scroll of its anchor, or a dialog', async () => {
    const { menu, pane } = await setup()
    const scroller = document.createElement('div')
    const anchor = document.createElement('div')
    scroller.append(anchor)
    document.body.append(scroller)
    const dismissals: (() => void)[] = [
      () => pane.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })),
      () => window.dispatchEvent(new Event('blur')),
      () => window.dispatchEvent(new Event('resize')),
      () => scroller.dispatchEvent(new Event('scroll')),
      () => {
        markDialogOpen('about')
      },
    ]
    for (const dismiss of dismissals) {
      menu.open({ rows: ROWS, at: { x: 40, y: 50 }, fromKeyboard: false, anchorElement: anchor })
      await settle()
      expect(surfaces()).toHaveLength(1)
      dismiss()
      await settle()
      expect(surfaces()).toHaveLength(0)
    }
    markDialogClosed('about')
  })

  it('stays open when something unrelated scrolls', async () => {
    const { menu } = await setup()
    const anchor = document.createElement('div')
    document.body.append(anchor)
    const elsewhere = document.createElement('div')
    document.body.append(elsewhere)
    menu.open({ rows: ROWS, at: { x: 40, y: 50 }, fromKeyboard: false, anchorElement: anchor })
    await settle()
    elsewhere.dispatchEvent(new Event('scroll'))
    await settle()
    expect(surfaces()).toHaveLength(1)
  })

  it('underlines its letters when the keyboard opened it, and only after a key when a click did', async () => {
    const { menu } = await setup()
    menu.open({ rows: ROWS, at: { x: 40, y: 50 }, fromKeyboard: true })
    await settle()
    expect(surfaces()[0].classList.contains('cues-shown')).toBe(true)
    menu.close()
    menu.open({ rows: ROWS, at: { x: 40, y: 50 }, fromKeyboard: false })
    await settle()
    expect(surfaces()[0].classList.contains('cues-shown')).toBe(false)
    await press('ArrowDown')
    expect(surfaces()[0].classList.contains('cues-shown')).toBe(true)
  })
})

describe('ContextMenu placement at the window edges', () => {
  const MENU = { width: 200, height: 150 }

  beforeEach(() => {
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
      return this.matches('[data-command-menu]') ? MENU.width : 0
    })
    vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockImplementation(function (this: HTMLElement) {
      return this.matches('[data-command-menu]') ? MENU.height : 0
    })
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  const { innerWidth: W, innerHeight: H } = window
  const cases: [string, { x: number; y: number }, { left: number; top: number }][] = [
    ['top left', { x: 10, y: 10 }, { left: 10, top: 10 }],
    ['top right', { x: W - 10, y: 10 }, { left: W - 10 - MENU.width, top: 10 }],
    ['bottom left', { x: 10, y: H - 10 }, { left: 10, top: H - 10 - MENU.height }],
    ['bottom right', { x: W - 10, y: H - 10 }, { left: W - 10 - MENU.width, top: H - 10 - MENU.height }],
  ]

  it.each(cases)('flips to stay inside the window at the %s corner', async (_corner, at, expected) => {
    const { menu } = await setup()
    menu.open({ rows: ROWS, at, fromKeyboard: false })
    await settle()
    const [surface] = surfaces()
    expect(surface.style.left).toBe(`${String(expected.left)}px`)
    expect(surface.style.top).toBe(`${String(expected.top)}px`)
  })

  it('scrolls a menu taller than the window', async () => {
    MENU.height = H * 2
    try {
      const { menu } = await setup()
      menu.open({ rows: ROWS, at: { x: 10, y: 10 }, fromKeyboard: false })
      await settle()
      expect(surfaces()[0].style.maxHeight).toBe(`${String(H - 4 - 10)}px`)
    } finally {
      MENU.height = 150
    }
  })
})
