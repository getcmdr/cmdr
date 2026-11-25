/**
 * `MainMenuButton` (task 63 step 8): the button that holds every bar menu as a submenu, on GNOME
 * (`primaryMenu`, with its standard group and no Quit) and on Windows and KDE where the bar is
 * hidden. Its keys (a press, Enter, ↓, F10 where it's the window's menu), the toggle, picks with
 * focus back where it was, a text field keeping focus, inert under a dialog, and axe.
 */

import { afterEach, describe, expect, it } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import type { BarMenuView, BarPickVia } from '#lib/ipc/bindings.js'
import { withConventions } from '#lib/platform/test-platform.js'
import type { ConventionsDesktop } from '#lib/platform/conventions.js'
import { expectNoA11yViolations } from '#lib/test-a11y.js'
import MainMenuButton from './MainMenuButton.svelte'
import { markDialogClosed, markDialogOpen } from './open-dialogs.svelte'

const item = (id: string, text: string) => ({
  kind: 'item' as const,
  id,
  label: { text },
  shortcut: null,
  ariaKeyshortcuts: null,
  enabled: true,
})

const MENUS: BarMenuView[] = [
  {
    id: 'menu_file',
    title: { text: 'File' },
    rows: [item('rename', 'Rename'), { kind: 'separator' }, item('quit', 'Quit')],
  },
  { id: 'menu_edit', title: { text: 'Edit' }, rows: [item('edit_copy', 'Copy'), item('settings', 'Settings')] },
  { id: 'menu_help', title: { text: 'Help' }, rows: [item('about', 'About'), item('help_shortcuts', 'Shortcuts')] },
]

const teardowns: (() => void)[] = []

afterEach(() => {
  for (const teardown of teardowns.splice(0)) teardown()
  document.body.replaceChildren()
})

async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) await tick()
}

/** Something outside the menu that had focus, to see focus come back to. */
function pane(): HTMLButtonElement {
  const button = document.createElement('button')
  button.textContent = 'Pane'
  document.body.append(button)
  button.focus()
  return button
}

async function setup(props: { desktop?: ConventionsDesktop; opensOnF10?: boolean } = {}) {
  teardowns.push(withConventions(props.desktop ?? 'gnome'))
  const picked: string[] = []
  const vias: BarPickVia[] = []
  /** The text field each pick says the menu was opened over, in `picked`'s order. */
  const fields: (HTMLElement | null)[] = []
  const target = document.createElement('div')
  document.body.append(target)
  const component = mount(MainMenuButton, {
    target,
    props: {
      menus: MENUS,
      onPick: (id: string, via: BarPickVia, textField: HTMLElement | null) => {
        picked.push(id)
        vias.push(via)
        fields.push(textField)
      },
      label: 'Main menu',
      opensOnF10: props.opensOnF10 ?? false,
    },
  })
  teardowns.push(() => {
    void unmount(component)
  })
  await settle()
  const button = target.querySelector<HTMLButtonElement>('.main-menu-button')
  if (!button) throw new Error('no button')
  return { picked, vias, fields, button }
}

function down(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init })
  ;(document.activeElement ?? document.body).dispatchEvent(event)
  return event
}

async function press(key: string, init: KeyboardEventInit = {}): Promise<KeyboardEvent> {
  const event = down(key, init)
  await settle()
  return event
}

async function click(element: HTMLElement): Promise<void> {
  element.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true, button: 0 }))
  element.click()
  await settle()
}

const surfaces = () => [...document.querySelectorAll<HTMLElement>('[data-command-menu="context"]')]
const rowTexts = (depth = 0) =>
  [...(surfaces()[depth]?.querySelectorAll<HTMLElement>('.command-menu-row') ?? [])].map((row) =>
    (row.querySelector('.command-menu-label')?.textContent ?? row.textContent).trim(),
  )

describe('MainMenuButton', () => {
  it('is a labelled button that pops up a menu, and passes axe', async () => {
    const { button } = await setup()
    expect(button.getAttribute('aria-label')).toBe('Main menu')
    expect(button.getAttribute('aria-haspopup')).toBe('menu')
    expect(button.getAttribute('aria-expanded')).toBe('false')
    await expectNoA11yViolations(document.body)
    await click(button)
    expect(button.getAttribute('aria-expanded')).toBe('true')
    await expectNoA11yViolations(document.body)
  })

  it('holds the bar’s menus as submenus, then GNOME’s standard group, with no Quit in File', async () => {
    pane()
    const { button } = await setup({ desktop: 'gnome' })
    await click(button)
    expect(rowTexts()).toEqual(['File', 'Edit', 'Help', 'Settings', 'Shortcuts', 'About'])
    await press('ArrowDown')
    await press('ArrowRight')
    expect(rowTexts(1)).toEqual(['Rename'])
  })

  it('stands in for a hidden bar on KDE with the bar’s menus alone, Quit kept', async () => {
    pane()
    const { button } = await setup({ desktop: 'kde' })
    await click(button)
    expect(rowTexts()).toEqual(['File', 'Edit', 'Help'])
    await press('ArrowDown')
    await press('ArrowRight')
    expect(rowTexts(1)).toEqual(['Rename', 'Quit'])
  })

  it('picks a row in a submenu by the keyboard and gives focus back where it was', async () => {
    const origin = pane()
    const { picked, button } = await setup()
    await click(button)
    await press('ArrowDown')
    await press('ArrowRight')
    await press('Enter')
    expect(picked).toEqual(['rename'])
    expect(surfaces()).toHaveLength(0)
    expect(document.activeElement).toBe(origin)
  })

  // Regression anchor (review of #1281): F10, `f`, `l` in a text field opened File > Delete, since
  // the menu's own keys counted as a click. The text-field gate runs a file-moving row only when
  // it was clicked, so the button says how each pick came.
  it('tells a pointer click on a row from the menu’s own keys', async () => {
    const field = document.createElement('input')
    document.body.append(field)
    field.focus()
    const { picked, vias, button } = await setup()
    await click(button)
    await press('ArrowDown')
    await press('ArrowRight')
    await press('Enter')
    expect(picked).toEqual(['rename'])
    expect(vias).toEqual(['key'])

    await click(button)
    await press('ArrowDown')
    await press('ArrowRight')
    const row = surfaces()[1]?.querySelector<HTMLElement>('[data-menu-id="rename"]')
    row?.click()
    await settle()
    expect(picked).toEqual(['rename', 'rename'])
    expect(vias).toEqual(['key', 'click'])
  })

  it('closes on a second press of the button, rather than reopening', async () => {
    pane()
    const { button } = await setup()
    await click(button)
    expect(surfaces()).toHaveLength(1)
    await click(button)
    expect(surfaces()).toHaveLength(0)
    expect(button.getAttribute('aria-expanded')).toBe('false')
  })

  it('opens from its own keys: Enter, Space, and ↓', async () => {
    const { button } = await setup()
    for (const key of ['Enter', ' ', 'ArrowDown']) {
      button.focus()
      await press(key)
      expect(surfaces(), key).toHaveLength(1)
      await press('Escape')
      expect(surfaces(), key).toHaveLength(0)
    }
  })

  it('opens on F10 from anywhere where it is the window’s menu, and leaves Shift+F10 alone', async () => {
    pane()
    await setup({ opensOnF10: true })
    const shifted = await press('F10', { shiftKey: true })
    expect(shifted.defaultPrevented).toBe(false)
    expect(surfaces()).toHaveLength(0)
    const plain = await press('F10')
    expect(plain.defaultPrevented).toBe(true)
    expect(surfaces()).toHaveLength(1)
  })

  it('leaves F10 to the bar where the bar takes it (Windows)', async () => {
    pane()
    await setup({ desktop: 'windows', opensOnF10: false })
    const event = await press('F10')
    expect(event.defaultPrevented).toBe(false)
    expect(surfaces()).toHaveLength(0)
  })

  it('leaves focus in a text field being edited, so the field never blurs', async () => {
    const field = document.createElement('input')
    document.body.append(field)
    field.focus()
    let blurs = 0
    field.addEventListener('blur', () => {
      blurs++
    })
    const { picked, fields, button } = await setup()
    await click(button)
    expect(document.activeElement).toBe(field)
    await press('ArrowDown')
    await press('ArrowRight')
    await press('Enter')
    expect(picked).toEqual(['rename'])
    expect(document.activeElement).toBe(field)
    expect(blurs).toBe(0)
    // The pick carries its target, the field the menu opened over (#1464).
    expect(fields).toEqual([field])

    pane()
    await click(button)
    await press('ArrowDown')
    await press('ArrowRight')
    await press('Enter')
    expect(fields).toEqual([field, null])
  })

  it('goes inert under a dialog, closing its menu', async () => {
    pane()
    const { button } = await setup({ opensOnF10: true })
    await click(button)
    markDialogOpen('about')
    teardowns.push(() => {
      markDialogClosed('about')
    })
    await settle()
    expect(surfaces()).toHaveLength(0)
    expect(button.hasAttribute('inert')).toBe(true)
    await press('F10')
    expect(surfaces()).toHaveLength(0)
  })
})
