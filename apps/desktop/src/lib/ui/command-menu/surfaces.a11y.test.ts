/**
 * Tier 3 a11y for the command menus' own surfaces, mounted without a bar or a context menu around
 * them: the stack (`MenuSurfaces`) with a submenu open, and one surface (`MenuSurface`) alone. The
 * owners' audits are `../command-menus.a11y.test.ts`.
 */

import { afterEach, describe, it } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import type { Row } from '#lib/ipc/bindings.js'
import { expectNoA11yViolations } from '#lib/test-a11y.js'
import MenuSurfaces from './MenuSurfaces.svelte'
import MenuSurface from './MenuSurface.svelte'
import { createMenuStack } from './menu-stack.svelte'

const ROWS: Row[] = [
  { kind: 'header', label: { text: 'report.pdf' } },
  { kind: 'item', id: 'open', label: { text: 'Open' }, shortcut: 'Enter', ariaKeyshortcuts: 'Enter', enabled: true },
  { kind: 'item', id: 'copy', label: { text: 'Copy' }, shortcut: null, ariaKeyshortcuts: null, enabled: false },
  { kind: 'separator' },
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
]

let teardowns: (() => void)[] = []

afterEach(() => {
  for (const teardown of teardowns.splice(0)) teardown()
  document.body.replaceChildren()
})

async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) await tick()
}

function stack() {
  return createMenuStack({ flavor: () => 'desktop', accessKeys: () => true, onPick: () => {} })
}

describe('command menu surfaces a11y', () => {
  it('a stack with its submenu open', async () => {
    const menu = stack()
    menu.open(ROWS, { kind: 'point', x: 10, y: 10 }, { fromKeyboard: true, label: 'report.pdf' })
    menu.walk(1)
    menu.enterSubmenu()
    const target = document.createElement('div')
    document.body.append(target)
    const component = mount(MenuSurfaces, {
      target,
      props: { stack: menu, flavor: 'desktop', cuesShown: true, owner: 'context' },
    })
    teardowns.push(() => {
      void unmount(component)
    })
    await settle()
    await expectNoA11yViolations(document.body)
  })

  it('one surface alone', async () => {
    const menu = stack()
    const surface = menu.open(ROWS, { kind: 'point', x: 10, y: 10 }, { fromKeyboard: false, label: 'report.pdf' })
    const target = document.createElement('div')
    document.body.append(target)
    const component = mount(MenuSurface, {
      target,
      props: {
        stack: menu,
        surface,
        depth: 0,
        flavor: 'mac',
        cuesShown: false,
        owner: 'context',
        lookClass: '',
        parentElement: undefined,
        childId: undefined,
      },
    })
    teardowns.push(() => {
      void unmount(component)
    })
    await settle()
    await expectNoA11yViolations(document.body)
  })
})
