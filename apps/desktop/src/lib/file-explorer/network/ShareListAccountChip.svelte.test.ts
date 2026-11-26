/**
 * The share list's account in the path bar: "as testuser" as a button whose menu holds the
 * server's account items, run by the one runner every door shares; and the same menu as a
 * share row's (Open, Copy path, the account items). ❗ No password crosses into it: the
 * frontend has no Keychain read at all.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, unmount, flushSync, tick } from 'svelte'

interface Hoisted {
  hosts: unknown[]
  listedAccount: { kind: 'user'; username: string } | { kind: 'guest' } | undefined
  guestListingWorked: boolean
  runServerAccountAction: (action: string, server: unknown) => Promise<{ listed: boolean }>
}

const h = vi.hoisted((): Hoisted & { runServerAccountAction: ReturnType<typeof vi.fn> } => ({
  hosts: [],
  listedAccount: undefined,
  guestListingWorked: false,
  runServerAccountAction: vi.fn((_action: string, _server: unknown) => Promise.resolve({ listed: false })),
}))

const saved = vi.hoisted(() => ({ runServerRowAction: vi.fn(), forgetSavedShare: vi.fn() }))

vi.mock('./network-store.svelte', () => ({ getNetworkHosts: () => h.hosts }))
vi.mock('../navigation/server-row-actions', () => ({ runServerRowAction: saved.runServerRowAction }))
vi.mock('./servers-hub-actions', () => ({ forgetSavedShare: saved.forgetSavedShare }))
vi.mock('./share-list-sign-in', () => ({
  runServerAccountAction: (action: string, server: unknown) => h.runServerAccountAction(action, server),
  serverAccountFacts: () => ({ listedAccount: h.listedAccount, guestListingWorked: h.guestListingWorked }),
}))

import ShareListAccountChip from './ShareListAccountChip.svelte'
import { shareListPath } from '#lib/servers/share-list-path.js'
import type { FileEntry, NetworkHost } from '../types'

const nas: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'testnas.local',
  port: 445,
  source: 'discovered',
}

let target: HTMLElement
let instance: ReturnType<typeof mount> | null = null
const onListed = vi.fn()
const onOpenChange = vi.fn((_change: { open: boolean; close: () => void }) => {})
const onOpenShare = vi.fn((_entry: FileEntry) => {})

interface ChipApi {
  openRowMenu: (entry: FileEntry, anchor: { x: number; y: number } | null | undefined) => void
}

function render(path = shareListPath(nas)): ChipApi {
  instance = mount(ShareListAccountChip, { target, props: { path, onListed, onOpenChange, onOpenShare } })
  flushSync()
  return instance as unknown as ChipApi
}

const publicShare: FileEntry = {
  name: 'public',
  path: 'smb://testnas/public',
  isDirectory: true,
  isSymlink: false,
  permissions: 0,
  owner: '',
  group: '',
  iconId: 'dir',
  shareMeta: { description: null, saved: null },
}

function menuRow(label: string): HTMLElement | undefined {
  return [...document.querySelectorAll<HTMLElement>('[data-menu] [data-menu-row]')].find(
    (el) => el.textContent.trim() === label,
  )
}

function button(): HTMLButtonElement | null {
  return target.querySelector('button.share-list-account')
}

function menuLabels(): string[] {
  return [...document.querySelectorAll('[data-menu] [data-menu-row]')].map((el) => el.textContent.trim())
}

async function openMenu(): Promise<void> {
  button()?.click()
  await vi.waitFor(() => {
    expect(document.querySelector('[data-menu]')).not.toBeNull()
  })
}

beforeEach(() => {
  vi.clearAllMocks()
  h.hosts = [nas]
  h.listedAccount = { kind: 'user', username: 'testuser' }
  h.guestListingWorked = false
  target = document.createElement('div')
  document.body.appendChild(target)
})

afterEach(() => {
  if (instance) void unmount(instance)
  instance = null
  target.remove()
})

describe('ShareListAccountChip', () => {
  it('names the account the share list is signed in as, and says what it opens', () => {
    render()
    expect(button()?.textContent.trim()).toBe('as testuser')
    expect(button()?.getAttribute('aria-haspopup')).toBe('menu')
    expect(button()?.getAttribute('aria-label')).toBe('Account on Testnas: as testuser')
  })

  it('offers Sign in as… itself while no listing has said who it’s signed in as', () => {
    h.listedAccount = undefined
    render()
    expect(button()?.textContent.trim()).toBe('Sign in as…')
  })

  it('shows nothing for a path that names no server it knows', () => {
    h.hosts = []
    render()
    expect(button()).toBeNull()
  })

  it('opens the server’s account items, and reports the menu to the header’s one openMenu', async () => {
    h.guestListingWorked = true
    render()
    await openMenu()
    expect(menuLabels()).toEqual(['Sign in as…', 'Browse as guest', 'Forget saved password'])
    expect(onOpenChange).toHaveBeenCalledWith(expect.objectContaining({ open: true }))
    expect(button()?.getAttribute('aria-expanded')).toBe('true')
  })

  it('runs a pick through the one runner, and re-lists the pane once it listed', async () => {
    h.runServerAccountAction.mockResolvedValueOnce({ listed: true })
    render()
    await openMenu()
    const signIn = [...document.querySelectorAll<HTMLElement>('[data-menu] [data-menu-row]')].find(
      (el) => el.textContent.trim() === 'Sign in as…',
    )
    signIn?.click()
    await tick()
    await vi.waitFor(() => {
      expect(onListed).toHaveBeenCalledOnce()
    })
    expect(h.runServerAccountAction).toHaveBeenCalledExactlyOnceWith('sign-in-as', { host: nas, label: 'Testnas' })
  })

  it('opens a share row’s menu at the key’s anchor: Open, Copy path, then the account items', async () => {
    const chip = render()
    chip.openRowMenu(publicShare, { x: 40, y: 80 })
    await vi.waitFor(() => {
      expect(document.querySelector('[data-menu]')).not.toBeNull()
    })
    expect(menuLabels()).toEqual(['Open', 'Copy path', 'Sign in as…', 'Forget saved password'])
    expect(onOpenChange).toHaveBeenCalledWith(expect.objectContaining({ open: true }))
  })

  it('opens a right-click’s row menu where the click landed, and Open mounts that share', async () => {
    const chip = render()
    document.dispatchEvent(new MouseEvent('contextmenu', { clientX: 12, clientY: 34, bubbles: true }))
    chip.openRowMenu(publicShare, undefined)
    await vi.waitFor(() => {
      expect(menuRow('Open')).toBeTruthy()
    })
    menuRow('Open')?.click()
    await tick()
    expect(onOpenShare).toHaveBeenCalledWith(publicShare)
  })

  it('gives a saved share its pin and Forget share, run on its place', async () => {
    const archive: FileEntry = {
      ...publicShare,
      name: 'archive',
      path: 'smb://testnas/archive',
      shareMeta: {
        description: null,
        saved: { volumeId: 'smb-testnas-archive', status: 'saved', account: null, pinned: false },
      },
    }
    const chip = render()
    chip.openRowMenu(archive, { x: 40, y: 80 })
    await vi.waitFor(() => {
      expect(menuRow('Pin to switcher')).toBeTruthy()
    })
    expect(menuLabels().slice(0, 4)).toEqual(['Open', 'Copy path', 'Pin to switcher', 'Forget share'])
    menuRow('Pin to switcher')?.click()
    await vi.waitFor(() => {
      expect(saved.runServerRowAction).toHaveBeenCalledWith({
        action: 'pin',
        volumeId: 'smb-testnas-archive',
        volumeName: 'archive',
      })
    })
    chip.openRowMenu(archive, { x: 40, y: 80 })
    await vi.waitFor(() => {
      expect(menuRow('Forget share')).toBeTruthy()
    })
    menuRow('Forget share')?.click()
    await vi.waitFor(() => {
      expect(saved.forgetSavedShare).toHaveBeenCalledWith('smb-testnas-archive', 'archive')
    })
  })

  it('has no menu for `..`, which is no share', async () => {
    const chip = render()
    chip.openRowMenu({ ...publicShare, name: '..', path: 'smb://' }, { x: 1, y: 1 })
    await tick()
    expect(document.querySelector('[data-menu]')).toBeNull()
  })
})
