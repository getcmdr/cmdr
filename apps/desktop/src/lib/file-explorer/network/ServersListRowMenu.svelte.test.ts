/**
 * `ServersListRowMenu.svelte`, the Servers list's row menu host: it opens a row's menu only
 * for a row with a server behind it, names the menu for that server, asks the mount table
 * (never the Keychain) on open, and hands Open back to the pane as Enter would.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import ServersListRowMenu from './ServersListRowMenu.svelte'
import { dirEntry } from '../views/test-full-list'
import type { FileEntry } from '../types'

const h = vi.hoisted(() => ({ smbHostHasMounts: vi.fn(() => Promise.resolve(false)) }))

vi.mock('./network-store.svelte', () => ({
  getNetworkHosts: () => [],
  getResolvedShareListHost: () => undefined,
  getListedAccount: () => undefined,
  guestListingWorked: () => false,
}))
vi.mock('#lib/stores/volume-store.svelte.js', () => ({ getVolumes: () => [] }))
vi.mock('#lib/tauri-commands/index.js', async (importOriginal) => ({
  ...(await importOriginal<typeof import('#lib/tauri-commands/index.js')>()),
  listSavedServers: vi.fn(() => Promise.resolve([])),
  smbHostHasMounts: h.smbHostHasMounts,
}))

/** A server Cmdr only found nearby, as the Servers list lists it. */
const testnas: FileEntry = dirEntry({
  name: 'Testnas',
  path: 'smb://testnas',
  serverMeta: {
    rowId: 'testnas',
    protocol: 'smb',
    address: 'testnas.local',
    status: 'found_nearby',
    account: null,
    saved: false,
    place: null,
    pinned: false,
  },
})

function mountMenu() {
  const target = document.createElement('div')
  document.body.appendChild(target)
  const onOpenChange = vi.fn()
  const onOpenRow = vi.fn()
  const component = mount(ServersListRowMenu, { target, props: { onOpenChange, onOpenRow } })
  return { component, onOpenChange, onOpenRow, done: () => unmount(component) }
}

/** The open menu's rows by their `data-menu-row` value. */
function menuRows(): string[] {
  return [...document.querySelectorAll<HTMLElement>('[data-menu-row]')].map((el) => el.dataset.menuRow ?? '')
}

beforeEach(() => {
  vi.clearAllMocks()
  document.body.innerHTML = ''
})

describe('ServersListRowMenu', () => {
  it('opens a server row’s menu at the anchor, named for the server, asking only the mount table', async () => {
    const { component, onOpenChange, done } = mountMenu()
    await component.openRowMenu(testnas, { x: 10, y: 20 })
    await tick()

    expect(onOpenChange).toHaveBeenLastCalledWith(expect.objectContaining({ open: true }))
    expect(h.smbHostHasMounts).toHaveBeenCalledOnce()
    expect(menuRows().length).toBeGreaterThan(0)
    expect(document.querySelector('[role="menu"]')?.getAttribute('aria-label')).toBe('Testnas')
    done()
  })

  it('opens nothing for a row with no server behind it, or with no anchor at all', async () => {
    const { component, onOpenChange, done } = mountMenu()
    await component.openRowMenu(dirEntry({ name: 'not-a-server', path: '/tmp/x' }), { x: 1, y: 1 })
    await component.openRowMenu(testnas, null)
    await tick()

    expect(onOpenChange).not.toHaveBeenCalledWith(expect.objectContaining({ open: true }))
    expect(menuRows()).toEqual([])
    done()
  })

  it('hands Open back to the pane, as Enter would', async () => {
    const { component, onOpenRow, done } = mountMenu()
    await component.openRowMenu(testnas, { x: 10, y: 20 })
    await tick()

    const open = [...document.querySelectorAll<HTMLElement>('[data-menu-row]')].find((el) =>
      (el.dataset.menuRow ?? '').endsWith(':open'),
    )
    expect(open, `an Open row among ${menuRows().join(', ')}`).toBeDefined()
    open?.click()
    await vi.waitFor(() => {
      expect(onOpenRow).toHaveBeenCalledWith(testnas)
    })
    done()
  })
})
