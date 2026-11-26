/**
 * NetworkMountView's own wiring, with the hub stubbed: what each hub callback turns into
 * for the pane (a place's volume change, a share list to open, the add sheet's three
 * answers), and the pass-through API FilePane and MCP drive. The real hub's behavior
 * (rows, keys, landing) is `NetworkMountView.test.ts` and `../network/ServersHub.test.ts`.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, unmount, tick } from 'svelte'
import NetworkMountView from './NetworkMountView.svelte'
import type { NetworkHost, VolumeInfo } from '../types'
import type { HubRow } from '../network/servers-hub-rows'
import type { VolumeChangePayload } from './types'

interface HubProps {
  onHostSelect: (host: NetworkHost) => void
  onServerSelect: (row: HubRow) => void
  onShareViaHost: (host: NetworkHost, share: { share: string; label: string }) => void
  onConnectToServer: (protocol?: string) => void
}

interface AddSheetOptions {
  protocol?: string
  onSmbHandOff: (handOff: { host: NetworkHost; sharePath: string | null }) => void
  onConnected: (place: { volumeId: string; root: string }) => void
  onAdded: (server: { serverId: string }) => void
}

const h = vi.hoisted(() => ({
  hubProps: null as HubProps | null,
  hubApi: {
    handleKeyDown: vi.fn(),
    setCursorIndex: vi.fn(),
    getItemCount: vi.fn(() => 3),
    jumpToPrefix: vi.fn((prefix: string) => (prefix === 'te' ? 'Testnas' : null)),
    findItemIndex: vi.fn((name: string) => (name === 'Testnas' ? 1 : -1)),
    getRowUnderCursor: vi.fn((): unknown => null),
    getHostUnderCursor: vi.fn((): unknown => null),
    openCursorItem: vi.fn(),
    openContextMenuAtCursor: vi.fn(() => Promise.resolve()),
    refresh: vi.fn(),
    selectServer: vi.fn(),
    selectShareListServer: vi.fn(),
  },
  volumes: [] as VolumeInfo[],
  openAddServerSheet: vi.fn((_options: unknown): Promise<unknown> => Promise.resolve({ kind: 'cancelled' })),
  focusHome: vi.fn(),
  warn: vi.fn(),
}))

// The hub as a bare component function: it records the props NetworkMountView hands it and
// answers `bind:this` with the API the view forwards to.
vi.mock('../network/ServersHub.svelte', () => ({
  default: (_anchor: unknown, props: HubProps) => {
    h.hubProps = props
    return h.hubApi
  },
}))
vi.mock('#lib/servers/open-sign-in.js', () => ({ openAddServerSheet: h.openAddServerSheet }))
vi.mock('#lib/ui/focus-restore.js', () => ({ focusHome: h.focusHome }))
vi.mock('#lib/stores/volume-store.svelte.js', () => ({ getVolumes: () => h.volumes }))
vi.mock('../network/network-store.svelte', () => ({ holdDiscoveryForServersView: () => () => {} }))
vi.mock('#lib/logging/logger.js', () => ({
  getAppLogger: () => ({ debug: vi.fn(), info: vi.fn(), warn: h.warn, error: vi.fn() }),
}))

const host: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'testnas.local',
  ipAddress: '192.168.50.10',
  port: 445,
  source: 'discovered',
}

function hubRow(overrides: Partial<HubRow>): HubRow {
  return {
    id: 'srv-1',
    kind: 'server',
    parentId: null,
    account: null,
    place: null,
    name: 'test_server_1',
    protocol: 'sftp',
    address: 'testserver.local',
    ...overrides,
  } as HubRow
}

interface ViewApi {
  handleKeyDown: (e: KeyboardEvent) => void
  setCursorIndex: (index: number) => void
  getItemCount: () => number
  jumpToPrefix: (prefix: string) => string | null
  findItemIndex: (name: string) => number
  getNetworkCursorEntry: () => unknown
  openCursorItem: () => void
  openContextMenuAtCursor: () => Promise<void>
  refreshNetworkHosts: () => void
}

function mountView(props: {
  onVolumeChange?: (change: VolumeChangePayload) => void
  onOpenServer?: (host: NetworkHost, arrival: { mount?: string }) => void
}) {
  const target = document.createElement('div')
  document.body.appendChild(target)
  const component = mount(NetworkMountView, { target, props: { paneId: 'left', isFocused: true, ...props } })
  if (!h.hubProps) throw new Error('the hub never rendered')
  return { component, api: component as unknown as ViewApi, hub: h.hubProps }
}

beforeEach(() => {
  vi.clearAllMocks()
  document.body.innerHTML = ''
  h.hubProps = null
  h.volumes = []
})

describe('NetworkMountView: what each hub callback opens', () => {
  it('opens a host as its share list, and a share via its host with that share to mount', async () => {
    const onOpenServer = vi.fn()
    const { component, hub } = mountView({ onOpenServer })

    hub.onHostSelect(host)
    hub.onShareViaHost(host, { share: 'photos', label: 'Testnas' })

    expect(onOpenServer).toHaveBeenNthCalledWith(1, host, {})
    expect(onOpenServer).toHaveBeenNthCalledWith(2, host, { mount: 'photos' })
    await unmount(component)
  })

  it('takes a live one-place server to its volume', async () => {
    h.volumes = [
      { id: 'sftp-1', path: 'sftp://testserver.local/home', connectionState: 'connected' } as unknown as VolumeInfo,
    ]
    const onVolumeChange = vi.fn()
    const { component, hub } = mountView({ onVolumeChange })

    hub.onServerSelect(hubRow({ volumeId: 'sftp-1' }))

    expect(onVolumeChange).toHaveBeenCalledWith({
      volumeId: 'sftp-1',
      volumePath: 'sftp://testserver.local/home',
      targetPath: 'sftp://testserver.local/home',
    })
    await unmount(component)
  })

  it('takes a saved place with no live volume to its place root, so the pane dials it', async () => {
    const onVolumeChange = vi.fn()
    const { component, hub } = mountView({ onVolumeChange })

    hub.onServerSelect(
      hubRow({ volumeId: 'sftp-2', place: { appRoot: 'sftp://testserver.local/srv' } as HubRow['place'] }),
    )

    expect(onVolumeChange).toHaveBeenCalledWith({
      volumeId: 'sftp-2',
      volumePath: 'sftp://testserver.local/srv',
      targetPath: 'sftp://testserver.local/srv',
    })
    await unmount(component)
  })

  it('logs and goes nowhere for a row with no place to open', async () => {
    const onVolumeChange = vi.fn()
    const { component, hub } = mountView({ onVolumeChange })

    hub.onServerSelect(hubRow({ volumeId: undefined }))

    expect(onVolumeChange).not.toHaveBeenCalled()
    expect(h.warn).toHaveBeenCalled()
    await unmount(component)
  })
})

describe('NetworkMountView: the add sheet', () => {
  it('hands each of its answers to the right door, then gives focus back', async () => {
    const onOpenServer = vi.fn()
    const onVolumeChange = vi.fn()
    const { component, hub } = mountView({ onOpenServer, onVolumeChange })

    hub.onConnectToServer('smb')
    await vi.waitFor(() => {
      expect(h.focusHome).toHaveBeenCalled()
    })
    const options = h.openAddServerSheet.mock.calls[0][0] as AddSheetOptions
    expect(options.protocol).toBe('smb')

    // An SMB address hands its host back: its share list opens, mounting the share it named.
    options.onSmbHandOff({ host, sharePath: 'photos' })
    options.onSmbHandOff({ host, sharePath: null })
    expect(onOpenServer).toHaveBeenNthCalledWith(1, host, { mount: 'photos' })
    expect(onOpenServer).toHaveBeenNthCalledWith(2, host, { mount: undefined })

    // A place that connected: the pane goes there.
    options.onConnected({ volumeId: 'sftp-3', root: 'sftp://testserver.local/' })
    expect(onVolumeChange).toHaveBeenCalledWith({
      volumeId: 'sftp-3',
      volumePath: 'sftp://testserver.local/',
      targetPath: 'sftp://testserver.local/',
    })

    // Saved without opening: the hub selects its new row.
    options.onAdded({ serverId: 'srv-9' })
    expect(h.hubApi.selectServer).toHaveBeenCalledWith('srv-9')
    await unmount(component)
  })
})

describe('NetworkMountView: the API FilePane and MCP drive', () => {
  it('forwards keys, the cursor, jumps, lookups, Enter, the row menu, and ⌘R to the hub', async () => {
    const { component, api } = mountView({})
    await tick()
    const key = new KeyboardEvent('keydown', { key: 'ArrowDown' })

    api.handleKeyDown(key)
    api.setCursorIndex(2)
    api.openCursorItem()
    await api.openContextMenuAtCursor()
    api.refreshNetworkHosts()

    expect(h.hubApi.handleKeyDown).toHaveBeenCalledWith(key)
    expect(h.hubApi.setCursorIndex).toHaveBeenCalledWith(2)
    expect(h.hubApi.openCursorItem).toHaveBeenCalled()
    expect(h.hubApi.openContextMenuAtCursor).toHaveBeenCalled()
    expect(h.hubApi.refresh).toHaveBeenCalled()
    expect(api.getItemCount()).toBe(3)
    expect(api.jumpToPrefix('te')).toBe('Testnas')
    expect(api.jumpToPrefix('zz')).toBeNull()
    expect(api.findItemIndex('Testnas')).toBe(1)
    expect(api.findItemIndex('Gone')).toBe(-1)
    await unmount(component)
  })

  it('names what is under the cursor: an SMB host, another server, or nothing', async () => {
    const { component, api } = mountView({})
    await tick()
    const row = hubRow({ protocol: 'smb' })

    expect(api.getNetworkCursorEntry()).toBeNull()

    h.hubApi.getRowUnderCursor.mockReturnValue(row)
    expect(api.getNetworkCursorEntry()).toEqual({ kind: 'server', row })

    h.hubApi.getHostUnderCursor.mockReturnValue(host)
    expect(api.getNetworkCursorEntry()).toEqual({ kind: 'host', host, row })
    await unmount(component)
  })
})
