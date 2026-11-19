/**
 * Tier-3 a11y for `InPageContextMenus`: a `show_*` menu (the rows hold focus) and a text field's
 * edit menu (the field holds focus, the cursor is its `aria-activedescendant`), each through axe.
 */

import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import type { Row } from '#lib/ipc/bindings.js'
import { withConventions } from '#lib/platform/test-platform.js'
import { expectNoA11yViolations } from '#lib/test-a11y.js'

const commands = vi.hoisted(() => ({
  openContextMenuInPage: vi.fn(),
  pickContextMenuItem: vi.fn(() => Promise.resolve('ran')),
}))

vi.mock('#lib/ipc/bindings.js', async (importOriginal) => {
  const actual = await importOriginal<typeof import('#lib/ipc/bindings.js')>()
  return { ...actual, commands: { ...actual.commands, ...commands } }
})

import { showTabContextMenu } from '#lib/tauri-commands/index.js'
import { installContextMenuOwnership } from '#lib/context-menu-ownership.js'
import InPageContextMenus from './InPageContextMenus.svelte'

const ROWS: Row[] = [
  { kind: 'item', id: 'tab_pin', label: { text: 'Pin tab' }, shortcut: null, ariaKeyshortcuts: null, enabled: true },
  { kind: 'separator' },
  {
    kind: 'item',
    id: 'tab_close',
    label: { text: 'Close tab' },
    shortcut: null,
    ariaKeyshortcuts: null,
    enabled: false,
  },
]

const teardowns: (() => void)[] = []

beforeEach(async () => {
  teardowns.push(withConventions('windows'))
  Object.defineProperty(document, 'queryCommandEnabled', { value: () => true, configurable: true })
  teardowns.push(installContextMenuOwnership())
  const target = document.createElement('div')
  document.body.append(target)
  const component = mount(InPageContextMenus, { target })
  teardowns.push(() => {
    void unmount(component)
  })
  await settle()
})

afterEach(() => {
  for (const teardown of teardowns.splice(0).reverse()) teardown()
  document.body.replaceChildren()
})

async function settle(): Promise<void> {
  for (let i = 0; i < 6; i++) await tick()
}

describe('InPageContextMenus a11y', () => {
  it('a keyboard-opened show_* menu passes axe, focus on its first row', async () => {
    commands.openContextMenuInPage.mockResolvedValue({ token: 1, rows: ROWS, anchor: { x: 20, y: 20 } })
    const tab = document.createElement('button')
    tab.textContent = 'Downloads'
    document.body.append(tab)
    tab.focus()
    tab.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true }))
    void showTabContextMenu(false, true, false)
    await settle()
    expect(document.activeElement?.getAttribute('role')).toBe('menuitem')
    await expectNoA11yViolations(document.body)
  })

  it("a text field's edit menu passes axe, the field keeping focus with the cursor as its active descendant", async () => {
    const input = document.createElement('input')
    input.type = 'text'
    input.value = 'hello'
    input.setAttribute('aria-label', 'Name')
    document.body.append(input)
    input.focus()
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true }))
    input.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 0 }))
    await settle()
    expect(document.activeElement).toBe(input)
    const active = document.getElementById(input.getAttribute('aria-activedescendant') ?? '')
    expect(active?.getAttribute('role')).toBe('menuitem')
    await expectNoA11yViolations(document.body)
  })
})
