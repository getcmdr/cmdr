/**
 * Tier 3 a11y for the command menus (`MenuBar`, `ContextMenu`): the closed bar, a flyout with
 * its submenu open, and a context menu, on the Windows flavor and the Mac one. The keyboard walk
 * and the states behind these roles are pinned in `MenuBar.svelte.test.ts` and
 * `ContextMenu.svelte.test.ts`.
 */

import { afterEach, describe, it } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import type { BarMenuView, Row } from '#lib/ipc/bindings.js'
import { withConventions } from '#lib/platform/test-platform.js'
import { expectNoA11yViolations } from '#lib/test-a11y.js'
import MenuBar from './MenuBar.svelte'
import ContextMenu from './ContextMenu.svelte'
import { createContextMenu } from './command-menu/context-menu.svelte'
import type { MenuFlavor } from './command-menu/rows'

const ROWS: Row[] = [
  {
    kind: 'item',
    id: 'open',
    label: { text: 'Open' },
    shortcut: 'Ctrl+O',
    ariaKeyshortcuts: 'Control+O',
    enabled: true,
  },
  { kind: 'item', id: 'save', label: { text: 'Save' }, shortcut: null, ariaKeyshortcuts: null, enabled: false },
  { kind: 'separator' },
  {
    kind: 'check',
    id: 'wrap',
    label: { text: 'Word wrap' },
    shortcut: null,
    ariaKeyshortcuts: null,
    enabled: true,
    checked: true,
    checkKind: 'checkbox',
  },
  {
    kind: 'submenu',
    id: 'sort',
    label: { text: 'Sort by' },
    enabled: true,
    rows: [
      {
        kind: 'check',
        id: 'name',
        label: { text: 'Name' },
        shortcut: null,
        ariaKeyshortcuts: null,
        enabled: true,
        checked: true,
        checkKind: 'radio',
      },
    ],
  },
  { kind: 'system', label: { text: 'Services' } },
]

const MENUS: BarMenuView[] = [
  { id: 'menu_file', title: { text: 'File' }, rows: ROWS },
  { id: 'menu_view', title: { text: 'View' }, rows: ROWS.slice(0, 1) },
]

const teardowns: (() => void)[] = []

afterEach(() => {
  for (const teardown of teardowns.splice(0)) teardown()
  document.body.replaceChildren()
})

async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) await tick()
}

function key(name: string, init: KeyboardEventInit = {}): void {
  ;(document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init }),
  )
}

describe.each(['desktop', 'mac'] as MenuFlavor[])('command menus a11y (%s)', (flavor) => {
  it('the closed bar', async () => {
    withConventions(flavor === 'mac' ? 'macos' : 'windows')
    const target = document.createElement('div')
    document.body.append(target)
    const component = mount(MenuBar, {
      target,
      props: { menus: MENUS, onPick: () => {}, ariaLabel: 'Menu bar', flavor },
    })
    teardowns.push(() => {
      void unmount(component)
    })
    await settle()
    await expectNoA11yViolations(document.body)
  })

  it('a flyout with its submenu open', async () => {
    withConventions(flavor === 'mac' ? 'macos' : 'windows')
    const target = document.createElement('div')
    document.body.append(target)
    const component = mount(MenuBar, {
      target,
      props: { menus: MENUS, onPick: () => {}, ariaLabel: 'Menu bar', flavor },
    })
    teardowns.push(() => {
      void unmount(component)
    })
    await settle()
    target
      .querySelector('[role="menuitem"]')
      ?.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }))
    await settle()
    key('End')
    key('ArrowUp')
    await settle()
    key('ArrowRight')
    await settle()
    await expectNoA11yViolations(document.body)
  })

  it('a context menu', async () => {
    withConventions(flavor === 'mac' ? 'macos' : 'windows')
    const menu = createContextMenu({ onPick: () => {}, flavor: () => flavor })
    const target = document.createElement('div')
    document.body.append(target)
    const component = mount(ContextMenu, { target, props: { menu } })
    teardowns.push(() => {
      void unmount(component)
    })
    menu.open({
      rows: [{ kind: 'header', label: { text: 'report.pdf' } }, ...ROWS],
      at: { x: 10, y: 10 },
      fromKeyboard: true,
    })
    await settle()
    await expectNoA11yViolations(document.body)
  })
})
