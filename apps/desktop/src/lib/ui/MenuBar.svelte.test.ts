/**
 * `MenuBar` and its flyouts (`command-menu/`), on the Windows flavor unless a test says
 * otherwise: every key D5 names (Alt tap, F10, Alt+letter, arrows between menus and into
 * submenus, Home/End, type-ahead by access key, Enter/Space, Escape one level at a time, focus
 * back where it was), the roles and states (axe), the keyboard-cue rule, the hover delay, and the
 * Mac flavor the browser demo draws.
 */

import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import type { BarMenuView, BarPickVia } from '#lib/ipc/bindings.js'
import { withConventions } from '#lib/platform/test-platform.js'
import { expectNoA11yViolations } from '#lib/test-a11y.js'
import MenuBar from './MenuBar.svelte'
import { registerFocusHome } from './focus-restore'
import { trapFocus } from './focus-trap'
import { markDialogClosed, markDialogOpen } from './open-dialogs.svelte'
import type { MenuFlavor } from './command-menu/rows'

const item = (id: string, text: string, extra: Partial<{ shortcut: string; enabled: boolean }> = {}) => ({
  kind: 'item' as const,
  id,
  label: { text },
  shortcut: extra.shortcut ?? null,
  ariaKeyshortcuts: extra.shortcut ? extra.shortcut.replace('Ctrl', 'Control') : null,
  enabled: extra.enabled ?? true,
})

const MENUS: BarMenuView[] = [
  {
    id: 'menu_file',
    title: { key: 'menu.bar.file', args: {} },
    rows: [
      item('file_copy', 'Copy', { shortcut: 'F5', enabled: false }),
      item('file_rename', 'Rename', { shortcut: 'Shift+F6' }),
      { kind: 'separator' },
      item('file_quit', 'Quit', { shortcut: 'Ctrl+Q' }),
    ],
  },
  {
    id: 'menu_view',
    title: { key: 'menu.bar.view', args: {} },
    rows: [
      {
        kind: 'check',
        id: 'show_hidden_files',
        label: { text: 'Show hidden files' },
        shortcut: 'Ctrl+Shift+.',
        ariaKeyshortcuts: 'Control+Shift+.',
        enabled: true,
        checked: true,
        checkKind: 'checkbox',
      },
      {
        kind: 'submenu',
        id: 'menu.view.sortBy',
        label: { text: 'Sort by' },
        enabled: true,
        rows: [
          {
            kind: 'check',
            id: 'sort_by_name',
            label: { text: 'Name' },
            shortcut: null,
            ariaKeyshortcuts: null,
            enabled: true,
            checked: true,
            checkKind: 'radio',
          },
          {
            kind: 'check',
            id: 'sort_by_size',
            label: { text: 'Size' },
            shortcut: null,
            ariaKeyshortcuts: null,
            enabled: true,
            checked: false,
            checkKind: 'radio',
          },
        ],
      },
      item('view_refresh', 'Refresh'),
    ],
  },
  { id: 'menu_run', title: { text: 'Run' }, rows: [item('run_now', 'Now')] },
]

const teardowns: (() => void)[] = []

afterEach(() => {
  for (const teardown of teardowns.splice(0)) teardown()
  document.body.replaceChildren()
  document.documentElement.classList.remove('underline-access-keys')
  vi.useRealTimers()
})

/** Something outside the bar that had focus, to see focus come back to. */
function pane(): HTMLButtonElement {
  const button = document.createElement('button')
  button.textContent = 'Pane'
  document.body.append(button)
  button.focus()
  return button
}

async function setup(
  props: {
    flavor?: MenuFlavor
    keyScope?: 'Viewer window'
    menus?: BarMenuView[]
    onFocusOrigin?: (origin: Element | null) => void
    revealOnly?: boolean
  } = {},
) {
  withConventions(props.flavor === 'mac' ? 'macos' : 'windows')
  const picked: string[] = []
  /** How each pick came, in `picked`'s order: a pointer click, or a key. */
  const vias: BarPickVia[] = []
  const target = document.createElement('div')
  document.body.append(target)
  const component = mount(MenuBar, {
    target,
    props: {
      menus: props.menus ?? MENUS,
      onPick: (id: string, via: BarPickVia) => {
        picked.push(id)
        vias.push(via)
      },
      ariaLabel: 'Menu bar',
      flavor: props.flavor ?? 'desktop',
      keyScope: props.keyScope,
      onFocusOrigin: props.onFocusOrigin,
      revealOnly: props.revealOnly ?? false,
    },
  })
  teardowns.push(() => {
    void unmount(component)
  })
  await settle()
  return { picked, vias, bar: target.querySelector<HTMLElement>('[role="menubar"]') }
}

/** Lets the placement pass and the focus effect run. */
async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) await tick()
}

function down(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init })
  ;(document.activeElement ?? document.body).dispatchEvent(event)
  return event
}

function up(key: string, init: KeyboardEventInit = {}): void {
  ;(document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true, ...init }))
}

async function press(key: string, init: KeyboardEventInit = {}): Promise<KeyboardEvent> {
  const event = down(key, init)
  up(key, init)
  await settle()
  return event
}

/** A lone Alt tap. */
async function tapAlt(): Promise<void> {
  down('Alt', { code: 'AltLeft', altKey: true })
  up('Alt', { code: 'AltLeft' })
  await settle()
}

const titles = () => [...document.querySelectorAll<HTMLElement>('.menu-bar-title')]
const surfaces = () => [...document.querySelectorAll<HTMLElement>('[data-command-menu="bar"]')]
const rows = (depth = 0) => [...(surfaces()[depth]?.querySelectorAll<HTMLElement>('.command-menu-row') ?? [])]
const focusedText = () => document.activeElement?.textContent.trim()

describe('MenuBar roles and states', () => {
  it('is a menubar of menuitem titles that each pop up a menu, and passes axe', async () => {
    const { bar } = await setup()
    expect(bar?.getAttribute('aria-label')).toBe('Menu bar')
    expect(titles().map((t) => t.textContent.trim())).toEqual(['File', 'View', 'Run'])
    for (const title of titles()) {
      expect(title.getAttribute('role')).toBe('menuitem')
      expect(title.getAttribute('aria-haspopup')).toBe('menu')
      expect(title.getAttribute('aria-expanded')).toBe('false')
    }
    expect(titles().map((t) => t.getAttribute('aria-keyshortcuts'))).toEqual(['Alt+F', 'Alt+V', 'Alt+R'])
    if (bar) await expectNoA11yViolations(bar)
  })

  it('names each row the way a menu does: checks, radios, greyed rows, shortcuts, and submenus', async () => {
    await setup()
    titles()[1].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    expect(titles()[1].getAttribute('aria-expanded')).toBe('true')
    expect(titles()[1].getAttribute('aria-controls')).toBe(surfaces()[0].id)
    expect(surfaces()[0].getAttribute('aria-labelledby')).toBe(titles()[1].id)
    const [hidden, sortBy] = rows()
    expect(hidden.getAttribute('role')).toBe('menuitemcheckbox')
    expect(hidden.getAttribute('aria-checked')).toBe('true')
    expect(hidden.getAttribute('aria-keyshortcuts')).toBe('Control+Shift+.')
    expect(hidden.querySelector('.command-menu-shortcut')?.textContent.trim()).toBe('Ctrl+Shift+.')
    expect(sortBy.getAttribute('aria-haspopup')).toBe('menu')
    expect(sortBy.getAttribute('aria-expanded')).toBe('false')
    await expectNoA11yViolations(document.body)

    titles()[0].dispatchEvent(new PointerEvent('pointerenter', { bubbles: true }))
    await settle()
    expect(rows()[0].getAttribute('aria-disabled')).toBe('true')
    expect(document.querySelector('[role="separator"]')).not.toBeNull()
  })

  it('draws a radio choice as menuitemradio', async () => {
    await setup()
    down('v', { code: 'KeyV', altKey: true })
    await settle()
    await press('ArrowDown')
    await press('ArrowRight')
    const [name, size] = rows(1)
    expect(name.getAttribute('role')).toBe('menuitemradio')
    expect(name.getAttribute('aria-checked')).toBe('true')
    expect(size.getAttribute('aria-checked')).toBe('false')
    await expectNoA11yViolations(document.body)
  })
})

describe('MenuBar keys on Windows', () => {
  // Regression anchor (review of #1265): the viewer's Cut and Paste read the search box's focus,
  // which the open menu took, so they were greyed in the very menu that offered them.
  it('tells its caller where focus was while it has the keyboard, and null once it lets go', async () => {
    const origin = pane()
    const heard: (Element | null)[] = []
    await setup({ onFocusOrigin: (element) => heard.push(element) })
    await tapAlt()
    await press('ArrowDown')
    expect(document.activeElement).not.toBe(origin)
    expect(heard.at(-1)).toBe(origin)
    await press('Escape')
    await press('Escape')
    expect(document.activeElement).toBe(origin)
    expect(heard.at(-1)).toBeNull()
  })

  it('takes the keyboard on a lone Alt tap and gives it back on the next', async () => {
    const origin = pane()
    await setup()
    await tapAlt()
    expect(document.activeElement).toBe(titles()[0])
    expect(titles()[0].tabIndex).toBe(0)
    await tapAlt()
    expect(document.activeElement).toBe(origin)
  })

  it("doesn't count an Alt that another key came between, nor AltGr", async () => {
    const origin = pane()
    await setup()
    down('Alt', { code: 'AltLeft', altKey: true })
    down('Tab', { altKey: true })
    up('Alt')
    await settle()
    expect(document.activeElement).toBe(origin)
    // AltGr on Windows: Ctrl then Alt, both down.
    down('AltGraph', { code: 'AltRight', altKey: true, ctrlKey: true })
    up('AltGraph', { code: 'AltRight' })
    await settle()
    expect(document.activeElement).toBe(origin)
  })

  it('resets Alt when the window loses focus mid-press (WebView2 sends no keyup then)', async () => {
    const origin = pane()
    await setup()
    down('Alt', { code: 'AltLeft', altKey: true })
    window.dispatchEvent(new Event('blur'))
    up('Alt')
    await settle()
    expect(document.activeElement).toBe(origin)
  })

  it('takes the keyboard on F10, and leaves Shift+F10 to the context menu', async () => {
    const origin = pane()
    await setup()
    const shifted = await press('F10', { shiftKey: true })
    expect(shifted.defaultPrevented).toBe(false)
    expect(document.activeElement).toBe(origin)
    const plain = await press('F10')
    expect(plain.defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(titles()[0])
  })

  it('opens a menu on Alt plus its letter, on its first live row, and leaves Alt+Space and Alt+F4 alone', async () => {
    pane()
    await setup()
    const space = down(' ', { code: 'Space', altKey: true })
    const f4 = down('F4', { altKey: true })
    expect(space.defaultPrevented || f4.defaultPrevented).toBe(false)
    expect(surfaces()).toHaveLength(0)
    const open = down('f', { code: 'KeyF', altKey: true })
    await settle()
    expect(open.defaultPrevented).toBe(true)
    expect(titles()[0].getAttribute('aria-expanded')).toBe('true')
    // Copy is greyed, so the cursor starts on Rename, with roving focus on it.
    expect(focusedText()).toContain('Rename')
    expect(document.activeElement?.getAttribute('tabindex')).toBe('0')
  })

  it('walks: ← and → between titles, ↓ opens, ↑/↓ wrap past greyed rows, Home and End jump', async () => {
    pane()
    await setup()
    await tapAlt()
    await press('ArrowRight')
    expect(document.activeElement).toBe(titles()[1])
    await press('ArrowLeft')
    await press('ArrowLeft')
    expect(document.activeElement).toBe(titles()[2])
    await press('Home')
    expect(document.activeElement).toBe(titles()[0])
    await press('ArrowDown')
    expect(focusedText()).toContain('Rename')
    await press('ArrowDown')
    expect(focusedText()).toContain('Quit')
    await press('ArrowDown')
    expect(focusedText()).toContain('Rename')
    await press('End')
    expect(focusedText()).toContain('Quit')
    await press('Home')
    expect(focusedText()).toContain('Rename')
  })

  it('opens ↑ on the last row, as Windows does', async () => {
    pane()
    await setup()
    await tapAlt()
    await press('ArrowUp')
    expect(focusedText()).toContain('Quit')
  })

  it('crosses to the neighbor menu with ← and →, and enters and leaves a submenu', async () => {
    pane()
    await setup()
    down('f', { code: 'KeyF', altKey: true })
    await settle()
    await press('ArrowRight')
    expect(titles()[1].getAttribute('aria-expanded')).toBe('true')
    expect(focusedText()).toContain('Show hidden files')
    await press('ArrowDown')
    await press('ArrowRight')
    expect(rows(1)).toHaveLength(2)
    expect(focusedText()).toContain('Name')
    expect(rows()[1].getAttribute('aria-expanded')).toBe('true')
    expect(rows()[1].getAttribute('aria-controls')).toBe(surfaces()[1].id)
    await press('ArrowLeft')
    expect(rows(1)).toHaveLength(0)
    expect(focusedText()).toContain('Sort by')
    await press('ArrowLeft')
    expect(titles()[0].getAttribute('aria-expanded')).toBe('true')
  })

  it('picks with Enter and Space, giving focus back first, and never picks a greyed row', async () => {
    const origin = pane()
    let focusedAtPick: Element | null = null
    const { picked } = await setup()
    const record = () => {
      focusedAtPick = document.activeElement
    }
    document.addEventListener('focusin', record)
    down('f', { code: 'KeyF', altKey: true })
    await settle()
    await press('Enter')
    expect(picked).toEqual(['file_rename'])
    expect(document.activeElement).toBe(origin)
    expect(focusedAtPick).toBe(origin)
    expect(surfaces()).toHaveLength(0)
    document.removeEventListener('focusin', record)

    down('v', { code: 'KeyV', altKey: true })
    await settle()
    await press(' ')
    expect(picked).toEqual(['file_rename', 'show_hidden_files'])

    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    rows()[0].click()
    expect(picked).toHaveLength(2)
  })

  // Regression anchor (review of #1281): from a text field, a stray Alt, then `f`, then `l` opened
  // File > Delete, because the menu's own keys counted as a click. The text-field gate refuses a
  // file-moving row reached by any key, so the bar says how each pick came.
  it('tells a pointer click from a key: Enter, Space, and a row’s letter are keys', async () => {
    const field = document.createElement('input')
    document.body.append(field)
    field.focus()
    const { picked, vias } = await setup()
    await tapAlt()
    await press('f')
    await press('Enter')
    down('v', { code: 'KeyV', altKey: true })
    await settle()
    await press(' ')
    expect(picked).toEqual(['file_rename', 'show_hidden_files'])
    expect(vias).toEqual(['key', 'key'])

    down('v', { code: 'KeyV', altKey: true })
    await settle()
    await press('b')
    await press('s')
    expect(vias.at(-1)).toBe('key')

    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 }))
    await settle()
    rows()[1].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 }))
    rows()[1].click()
    await settle()
    expect(picked.at(-1)).toBe('file_rename')
    expect(vias.at(-1)).toBe('click')

    // A menu a click opened, then the keyboard: still a key.
    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 }))
    await settle()
    await press('ArrowDown')
    await press('Enter')
    expect(vias.at(-1)).toBe('key')
  })

  it('picks by a row’s letter, entering a submenu by its letter too', async () => {
    pane()
    const { picked } = await setup()
    down('v', { code: 'KeyV', altKey: true })
    await settle()
    // "Show hidden files" takes s, "Sort by" the next free initial, b; Refresh r.
    await press('b')
    expect(rows(1)).toHaveLength(2)
    await press('s')
    expect(picked).toEqual(['sort_by_size'])
  })

  it('backs out with Escape one level at a time, then gives focus back', async () => {
    const origin = pane()
    await setup()
    down('v', { code: 'KeyV', altKey: true })
    await settle()
    await press('ArrowDown')
    await press('ArrowRight')
    expect(surfaces()).toHaveLength(2)
    await press('Escape')
    expect(surfaces()).toHaveLength(1)
    await press('Escape')
    expect(surfaces()).toHaveLength(0)
    expect(document.activeElement).toBe(titles()[1])
    await press('Escape')
    expect(document.activeElement).toBe(origin)
  })

  it('keeps every key it has from the app behind it', async () => {
    pane()
    await setup()
    const appKeys: string[] = []
    const listener = (event: KeyboardEvent) => appKeys.push(event.key)
    document.addEventListener('keydown', listener)
    try {
      await tapAlt()
      await press('ArrowDown')
      await press('x')
      await press('Escape')
      // A lone Alt goes on to the page, which must still see its keyup (task 63 step 6's guard).
      expect(appKeys).toEqual(['Alt'])
    } finally {
      document.removeEventListener('keydown', listener)
    }
  })

  it("gives a title no letter the window's own Alt keys hold (the viewer's Alt+R)", async () => {
    await setup({ keyScope: 'Viewer window' })
    // Run can't take r (Alt+R is the viewer's), so it takes u.
    expect(titles()[2].getAttribute('aria-keyshortcuts')).toBe('Alt+U')
  })

  it('goes inert while a dialog is up, closing its menu', async () => {
    const origin = pane()
    await setup()
    down('f', { code: 'KeyF', altKey: true })
    await settle()
    markDialogOpen('about')
    teardowns.push(() => {
      markDialogClosed('about')
    })
    await settle()
    expect(surfaces()).toHaveLength(0)
    origin.focus()
    await tapAlt()
    expect(document.activeElement).toBe(origin)
    expect(document.querySelector('[role="menubar"]')?.hasAttribute('inert')).toBe(true)
  })

  // Regression anchor (review of #1271): an inert bar over the rename sent a title click to the
  // page, which the rename read as a click away, so the Edit rows never reached its field.
  it('stays live over the inline rename, a soft dialog only to be seen', async () => {
    pane()
    await setup()
    markDialogOpen('rename')
    teardowns.push(() => {
      markDialogClosed('rename')
    })
    await settle()
    expect(document.querySelector('[role="menubar"]')?.hasAttribute('inert')).toBe(false)
    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 }))
    await settle()
    expect(surfaces()).toHaveLength(1)
  })

  it('closes on a click elsewhere, without moving focus', async () => {
    const origin = pane()
    await setup()
    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    expect(surfaces()).toHaveLength(1)
    origin.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    expect(surfaces()).toHaveLength(0)
  })

  // Regression anchor (review of #1271): the menu took focus from the inline rename's field, whose
  // blur ended the rename before the Edit row it was opened for could act on it.
  it('leaves focus in a text field the whole time, by a click or by the keyboard, so the field never blurs', async () => {
    const field = document.createElement('input')
    document.body.append(field)
    field.focus()
    let blurs = 0
    field.addEventListener('blur', () => {
      blurs++
    })
    const { picked } = await setup()

    // A click on a title, a click on a row.
    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 }))
    await settle()
    expect(surfaces()).toHaveLength(1)
    expect(document.activeElement).toBe(field)
    rows()[1].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 }))
    rows()[1].click()
    await settle()
    expect(picked).toEqual(['file_rename'])

    // Alt+letter, a walk, Escape back to the title (its cursor is the field's active descendant), Enter.
    down('f', { code: 'KeyF', altKey: true })
    await settle()
    expect(document.activeElement).toBe(field)
    expect(field.getAttribute('aria-activedescendant')).toBe(rows()[1].id)
    const typed = await press('x')
    expect(typed.defaultPrevented).toBe(true)
    await press('Escape')
    expect(field.getAttribute('aria-activedescendant')).toBe(titles()[0].id)
    expect(titles()[0].classList.contains('is-keyboard')).toBe(true)
    await press('Enter')
    await press('Enter')
    expect(picked).toEqual(['file_rename', 'file_rename'])

    expect(document.activeElement).toBe(field)
    expect(field.hasAttribute('aria-activedescendant')).toBe(false)
    expect(blurs).toBe(0)
  })

  it('opens a hovered submenu after the menu delay, not at once', async () => {
    pane()
    await setup()
    titles()[1].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    vi.useFakeTimers()
    rows()[1].dispatchEvent(new PointerEvent('pointerenter'))
    await settle()
    expect(surfaces()).toHaveLength(1)
    vi.advanceTimersByTime(400)
    await settle()
    expect(surfaces()).toHaveLength(2)
  })

  it('follows the pointer between titles once a menu is open', async () => {
    await setup()
    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    titles()[2].dispatchEvent(new PointerEvent('pointerenter'))
    await settle()
    expect(titles()[2].getAttribute('aria-expanded')).toBe('true')
    expect(rows().map((r) => r.textContent.trim())).toEqual(['Now'])
  })
})

describe('MenuBar hidden until Alt (Windows, "Show menu bar" off)', () => {
  const collapsed = () => document.querySelector('[role="menubar"]')?.classList.contains('collapsed')

  it('stays hidden at rest, shows on an Alt tap, and hides again as it gives the keyboard back', async () => {
    const origin = pane()
    await setup({ revealOnly: true })
    expect(collapsed()).toBe(true)
    await tapAlt()
    expect(collapsed()).toBe(false)
    expect(document.activeElement).toBe(titles()[0])
    await press('Escape')
    expect(collapsed()).toBe(true)
    expect(document.activeElement).toBe(origin)
  })

  it('shows on F10, and opens a menu on Alt plus its letter with the bar laid out', async () => {
    pane()
    await setup({ revealOnly: true })
    await press('F10')
    expect(collapsed()).toBe(false)
    await press('Escape')
    expect(collapsed()).toBe(true)
    down('f', { code: 'KeyF', altKey: true })
    await settle()
    expect(collapsed()).toBe(false)
    expect(titles()[0].getAttribute('aria-expanded')).toBe('true')
    expect(focusedText()).toContain('Rename')
  })

  it('hides again when a click elsewhere takes the keyboard away', async () => {
    pane()
    await setup({ revealOnly: true })
    await tapAlt()
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    expect(collapsed()).toBe(true)
  })
})

describe('MenuBar keyboard cues (Win32)', () => {
  const underlined = () => document.querySelector('.menu-bar')?.classList.contains('cues-shown') ?? false
  const menuUnderlined = () => surfaces()[0]?.classList.contains('cues-shown') ?? false

  it('underlines the titles while Alt is held, and keeps them in a bar Alt opened', async () => {
    pane()
    await setup()
    expect(underlined()).toBe(false)
    down('Alt', { code: 'AltLeft', altKey: true })
    await settle()
    expect(underlined()).toBe(true)
    up('Alt')
    await settle()
    expect(underlined()).toBe(true)
    await press('ArrowDown')
    expect(menuUnderlined()).toBe(true)
  })

  it('hides them in a menu a click opened, until a key is pressed', async () => {
    pane()
    await setup()
    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    expect(menuUnderlined()).toBe(false)
    await press('ArrowDown')
    expect(menuUnderlined()).toBe(true)
  })

  it("shows them always under Windows' Underline access keys setting", async () => {
    document.documentElement.classList.add('underline-access-keys')
    await setup()
    expect(underlined()).toBe(true)
    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    expect(menuUnderlined()).toBe(true)
  })

  it('underlines the letter it allocated, in the label as shown', async () => {
    await setup()
    const key = titles()[1].querySelector('.menu-bar-access-key')
    expect(key?.textContent).toBe('V')
  })
})

/**
 * The window's file list inside a registered focus home, as the explorer registers it, with a
 * refocus that puts focus on it: what the app does when it re-homes focus on its own.
 */
function explorerList(): HTMLElement {
  const root = document.createElement('div')
  const list = document.createElement('div')
  list.tabIndex = 0
  list.textContent = 'List'
  root.append(list)
  document.body.append(root)
  teardowns.push(
    registerFocusHome({
      root: () => root,
      refocus: () => {
        list.focus()
      },
    }),
  )
  return list
}

describe('MenuBar and the app moving focus on its own', () => {
  it('keeps the bar on the keyboard when the app puts focus back on its list right after ⌃F2', async () => {
    const list = explorerList()
    list.focus()
    await setup({ flavor: 'mac' })
    await press('F2', { ctrlKey: true })
    expect(document.activeElement).toBe(titles()[0])
    // The explorer's own refocus runs after the title took focus (a fresh page's first ⌃F2).
    list.focus()
    await settle()
    expect(document.activeElement).toBe(titles()[0])
    await press('ArrowRight')
    expect(document.activeElement).toBe(titles()[1])
    await press('ArrowDown')
    expect(surfaces()).toHaveLength(1)
  })

  it('keeps a menu a click opened while focus was adrift, when the app re-homes focus to its list', async () => {
    const list = explorerList()
    await setup({ flavor: 'mac' })
    ;(document.activeElement as HTMLElement | null)?.blur()
    expect(document.activeElement).toBe(document.body)
    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    // Focus goes from `<body>` to the list, never through the bar.
    list.focus()
    await settle()
    expect(surfaces()).toHaveLength(1)
    expect(titles()[0].getAttribute('aria-expanded')).toBe('true')
  })

  it('lets go when focus leaves the bar for somewhere else (a text field a command opened)', async () => {
    pane()
    await setup()
    await tapAlt()
    expect(document.activeElement).toBe(titles()[0])
    const field = document.createElement('input')
    document.body.append(field)
    field.focus()
    await settle()
    expect(titles()[0].getAttribute('tabindex')).toBe('-1')
    await press('ArrowRight')
    expect(document.activeElement).toBe(field)
  })
})

describe('MenuBar on the Mac flavor (the browser demo)', () => {
  it('takes the bar on ⌃F2 and ignores Alt, F10, and Alt+letter', async () => {
    const origin = pane()
    await setup({ flavor: 'mac' })
    await tapAlt()
    await press('F10')
    down('f', { code: 'KeyF', altKey: true })
    await settle()
    expect(document.activeElement).toBe(origin)
    expect(surfaces()).toHaveLength(0)
    await press('F2', { ctrlKey: true })
    expect(document.activeElement).toBe(titles()[0])
    expect(titles()[0].matches(':first-child')).toBe(true)
    expect(titles()[0].getAttribute('aria-keyshortcuts')).toBeNull()
  })

  it('opens a hovered submenu at once, draws glyph shortcuts, and checks a radio choice', async () => {
    await setup({
      flavor: 'mac',
      menus: [
        { ...MENUS[1], rows: MENUS[1].rows.map((row) => (row.kind === 'check' ? { ...row, shortcut: '⇧⌘.' } : row)) },
      ],
    })
    titles()[0].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    expect(rows()[0].querySelectorAll('.command-menu-shortcut-glyph')).toHaveLength(3)
    rows()[1].dispatchEvent(new PointerEvent('pointerenter'))
    await settle()
    expect(surfaces()).toHaveLength(2)
    expect(rows(1)[0].querySelector('circle')).toBeNull()
    expect(rows(1)[0].querySelector('path')).not.toBeNull()
  })

  it('stays usable while a dialog is up, as macOS keeps its bar over a sheet', async () => {
    // A trapping dialog, as `ModalDialog` renders one, holding focus.
    const dialog = document.createElement('div')
    dialog.setAttribute('role', 'dialog')
    const field = document.createElement('input')
    dialog.append(field)
    document.body.append(dialog)
    const trap = trapFocus(dialog)
    field.focus()
    markDialogOpen('about')
    teardowns.push(() => {
      markDialogClosed('about')
      trap.destroy?.()
    })
    await setup({ flavor: 'mac' })
    expect(document.querySelector('[role="menubar"]')?.hasAttribute('inert')).toBe(false)
    titles()[1].dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    await Promise.resolve()
    await settle()
    expect(surfaces()).toHaveLength(1)
    await press('ArrowDown')
    // The dialog's field keeps focus; the cursor is its active descendant.
    expect(document.activeElement).toBe(field)
    const cursor = document.getElementById(field.getAttribute('aria-activedescendant') ?? '')
    expect(cursor?.textContent).toContain('Show hidden files')
    await press('Escape')
    await press('Escape')
    expect(document.activeElement).toBe(field)
  })

  it('opens ↑ on the first row, as the Mac does', async () => {
    pane()
    await setup({ flavor: 'mac' })
    await press('F2', { ctrlKey: true })
    await press('ArrowUp')
    expect(focusedText()).toContain('Rename')
  })
})
