/**
 * The share list's account in the path bar: "as testuser" as a button whose menu holds the
 * server's account items, run by the one runner every door shares. ❗ It reads no password.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, unmount, flushSync, tick } from 'svelte'

interface Hoisted {
  hosts: unknown[]
  listedAccount: { kind: 'user'; username: string } | { kind: 'guest' } | undefined
  guestListingWorked: boolean
  runServerAccountAction: (action: string, server: unknown) => Promise<{ listed: boolean }>
  getSmbCredentials: () => Promise<never>
}

const h = vi.hoisted(
  (): Hoisted & { runServerAccountAction: ReturnType<typeof vi.fn>; getSmbCredentials: ReturnType<typeof vi.fn> } => ({
    hosts: [],
    listedAccount: undefined,
    guestListingWorked: false,
    runServerAccountAction: vi.fn((_action: string, _server: unknown) => Promise.resolve({ listed: false })),
    getSmbCredentials: vi.fn(() => Promise.reject(new Error('never read here'))),
  }),
)

vi.mock('./network-store.svelte', () => ({ getNetworkHosts: () => h.hosts }))
vi.mock('./share-list-sign-in', () => ({
  runServerAccountAction: (action: string, server: unknown) => h.runServerAccountAction(action, server),
  serverAccountFacts: () => ({ listedAccount: h.listedAccount, guestListingWorked: h.guestListingWorked }),
}))
vi.mock('#lib/tauri-commands/index.js', () => ({ getSmbCredentials: h.getSmbCredentials }))

import ShareListAccountChip from './ShareListAccountChip.svelte'
import { shareListPath } from '#lib/servers/share-list-path.js'
import type { NetworkHost } from '../types'

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

function render(path = shareListPath(nas)): void {
  instance = mount(ShareListAccountChip, { target, props: { path, onListed, onOpenChange } })
  flushSync()
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
    // ❗ No password read to decide any of it.
    expect(h.getSmbCredentials).not.toHaveBeenCalled()
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
})
