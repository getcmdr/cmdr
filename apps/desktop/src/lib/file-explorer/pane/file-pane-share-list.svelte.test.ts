/**
 * A pane on a server's share list (`network` at `smb://<server>`) is a real listing the
 * standard views render, and its `..` goes to the Servers hub with the cursor on THAT
 * server's row. No door opens a share list yet (the navigation refusals stand), so this
 * starts the pane there the way a test can: as its tab's path.
 */
import { describe, it, expect, vi } from 'vitest'
import { mount, unmount } from 'svelte'
import FilePane from './FilePane.svelte'
import { waitForUpdates, useMountTarget, type ListenRecorder } from './integration-test-utils'
import type { FilePaneAPI } from './types'
import type { FileEntry, NetworkHost } from '../types'
import { explorerState } from './explorer-state.svelte'
import { createInitialTabState } from './tab-operations'
import { createTabManager, getActiveTab } from '../tabs/tab-state-manager.svelte'
import { writeTabPath } from './hub-landing.svelte'

const events = vi.hoisted(() => ({ recorder: null as ListenRecorder | null }))
const listDirectoryStartMock = vi.hoisted(() => vi.fn())
const net = vi.hoisted(() => ({ hosts: [] as NetworkHost[] }))

/** The fixture server's shares, as the backend lists them: one folder row per share. */
const SHARES = vi.hoisted(() =>
  ['public', 'media'].map((name) => ({
    name,
    path: `smb://testnas/${name}`,
    isDirectory: true,
    isSymlink: false,
    size: null,
    modifiedAt: null,
    iconId: 'dir',
    permissions: 0,
    owner: '',
    group: '',
    extendedMetadataLoaded: true,
    tags: [],
    shareMeta: { description: `The ${name} share` },
  })),
)

vi.mock('#lib/tauri-commands/index.js', async (importOriginal) => {
  const { createListenRecorder } = await import('./integration-test-utils')
  const recorder = createListenRecorder()
  events.recorder = recorder
  return {
    ...(await importOriginal<typeof import('#lib/tauri-commands/index.js')>()),
    listDirectoryStart: listDirectoryStartMock,
    cancelListing: vi.fn().mockResolvedValue(undefined),
    listDirectoryEnd: vi.fn().mockResolvedValue(undefined),
    onListingGone: vi.fn(() => () => {}),
    keepListingsAlive: vi.fn().mockResolvedValue([]),
    getFileRange: vi.fn((_id: string, start: number, count: number) =>
      Promise.resolve(SHARES.slice(start, start + count) as unknown as FileEntry[]),
    ),
    getFileAt: vi.fn((_id: string, index: number) => Promise.resolve(SHARES[index] ?? null)),
    findFileIndex: vi.fn().mockResolvedValue(null),
    getTotalCount: vi.fn().mockResolvedValue(SHARES.length),
    getListingStats: vi.fn().mockResolvedValue({ data: null, timedOut: false }),
    setListingIncludeHidden: vi
      .fn()
      .mockResolvedValue({ sequence: 0, totalCount: 0, newCursorIndex: null, newSelectedIndices: null }),
    getSyncStatus: vi.fn().mockResolvedValue({ data: {}, timedOut: false }),
    onMediaEnrichProgress: vi.fn().mockResolvedValue(() => {}),
    onMediaEnrichTerminal: vi.fn().mockResolvedValue(() => {}),
    listen: recorder.listen,
    ...recorder.listingWrappers,
    showFileContextMenu: vi.fn().mockResolvedValue(undefined),
    updateMenuContext: vi.fn().mockResolvedValue(undefined),
    updateServicesSelection: vi.fn().mockResolvedValue(undefined),
    updateSelectSameKindMenu: vi.fn().mockResolvedValue(undefined),
    listVolumes: vi.fn().mockResolvedValue({ data: [], timedOut: false }),
    resolvePathVolume: vi.fn().mockResolvedValue({ volume: null, timedOut: false }),
    getDefaultVolumeId: vi.fn().mockResolvedValue('root'),
    getVolumeSpace: vi.fn().mockResolvedValue({ data: null, timedOut: false }),
    refreshListing: vi.fn().mockResolvedValue({ data: null, timedOut: false }),
    getIcons: vi.fn().mockResolvedValue({ data: {}, timedOut: false }),
    refreshDirectoryIcons: vi.fn().mockResolvedValue({ data: {}, timedOut: false }),
    DEFAULT_VOLUME_ID: 'root',
    listSavedServers: vi.fn().mockResolvedValue([]),
    setServersViewShown: vi.fn().mockResolvedValue(undefined),
    listMtpDevices: vi.fn().mockResolvedValue([]),
    onMtpDeviceConnected: vi.fn().mockResolvedValue(() => {}),
    onMtpDeviceDisconnected: vi.fn().mockResolvedValue(() => {}),
    onVolumeSpaceChanged: vi.fn().mockResolvedValue(() => {}),
    onWriteSourceItemDone: vi.fn().mockResolvedValue(() => {}),
    onDirectoryDiff: vi.fn().mockResolvedValue(() => {}),
    onDirectoryDeleted: vi.fn().mockResolvedValue(() => {}),
    onListingRespelled: vi.fn().mockResolvedValue(() => {}),
    onMtpExclusiveAccessError: vi.fn().mockResolvedValue(() => {}),
    onMtpPermissionError: vi.fn().mockResolvedValue(() => {}),
    onVolumeContextAction: vi.fn().mockResolvedValue(() => {}),
    notifyDialogOpened: vi.fn().mockResolvedValue(undefined),
    notifyDialogClosed: vi.fn().mockResolvedValue(undefined),
    watchVolumeSpace: vi.fn().mockResolvedValue(undefined),
    unwatchVolumeSpace: vi.fn().mockResolvedValue(undefined),
  }
})

vi.mock('../network/network-store.svelte', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../network/network-store.svelte')>()),
  getNetworkHosts: () => net.hosts,
  getDiscoveryState: () => 'idle',
  holdDiscoveryForServersView: () => () => {},
  refreshAllStaleShares: vi.fn(),
}))

vi.mock('#lib/icon-cache.js', async (importOriginal) => {
  const { writable } = await import('svelte/store')
  return {
    ...(await importOriginal<typeof import('#lib/icon-cache.js')>()),
    getCachedIcon: vi.fn().mockReturnValue('/icons/file.png'),
    getCachedCustomFolderIcon: () => undefined,
    iconCacheVersion: writable(0),
    prefetchIcons: vi.fn().mockResolvedValue(undefined),
    prefetchCustomFolderIcons: vi.fn().mockResolvedValue(undefined),
    evictPerPathIconsForDir: vi.fn(),
  }
})

vi.mock('#lib/settings/reactive-settings.svelte.js', async (importOriginal) => ({
  ...(await importOriginal<typeof import('#lib/settings/reactive-settings.svelte.js')>()),
  getRowHeight: vi.fn().mockReturnValue(24),
  getUseAppIconsForDocuments: vi.fn().mockReturnValue(true),
  getNetworkEnabled: vi.fn().mockReturnValue(true),
  getMediaIndexEnabled: vi.fn().mockReturnValue(false),
  getMediaIndexShowFileStatusIcons: vi.fn().mockReturnValue(false),
  getShowVirtualGitPortal: vi.fn().mockReturnValue(true),
}))

vi.mock('#lib/drag-drop', () => ({ startDragTracking: vi.fn() }))

vi.mock('#lib/stores/volume-store.svelte.js', () => ({
  getVolumes: vi.fn().mockReturnValue([]),
  getVolumesTimedOut: vi.fn().mockReturnValue(false),
  isVolumesRefreshing: vi.fn().mockReturnValue(false),
  isVolumeRetryFailed: vi.fn().mockReturnValue(false),
  requestVolumeRefresh: vi.fn(),
  initVolumeStore: vi.fn().mockResolvedValue(undefined),
  cleanupVolumeStore: vi.fn(),
}))

const ATTIC: NetworkHost = { id: 'attic-id', name: 'Attic', hostname: 'attic.local', port: 445, source: 'discovered' }
const TESTNAS: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'testnas.local',
  port: 445,
  source: 'discovered',
}

function lastListingId(): string {
  const calls = listDirectoryStartMock.mock.calls
  return calls[calls.length - 1][5] as string
}

describe("FilePane on a server's share list", () => {
  const { getTarget } = useMountTarget()

  it('renders the shares as a listing, and `..` lands on the hub with the cursor on that server', async () => {
    net.hosts = [ATTIC, TESTNAS]
    listDirectoryStartMock.mockImplementation((_v, _p, _h, _sb, _so, listingId: string) =>
      Promise.resolve({ listingId, status: { status: 'loading' } }),
    )
    // The tab DualPaneExplorer would hold; `onPathChange` commits through the same
    // one path write `pane-accessors.svelte.ts::setPanePath` makes.
    explorerState.setTabMgr('left', createTabManager(createInitialTabState('smb://testnas', 'network')))
    // and hands the pane its tab's path back, as DualPaneExplorer does.
    const committed: string[] = []
    const props = $state({
      paneId: 'left' as const,
      initialPath: 'smb://testnas',
      volumeId: 'network',
      volumePath: 'smb://',
      isFocused: true,
      showHiddenFiles: true,
      viewMode: 'full' as const,
      onPathChange: (path: string) => {
        committed.push(path)
        writeTabPath(getActiveTab(explorerState.getTabMgr('left')), path)
        props.initialPath = path
      },
    })
    const component = mount(FilePane, { target: getTarget(), props })
    const pane = component as unknown as FilePaneAPI
    await waitForUpdates()

    // The backend listing ran on the share list's own path, and its rows render.
    expect(listDirectoryStartMock.mock.calls[0][1]).toBe('smb://testnas')
    events.recorder?.fireListingEvent('listing-complete', {
      listingId: lastListingId(),
      totalCount: SHARES.length,
      volumeRoot: 'smb://',
    })
    await waitForUpdates(100)
    // The standard list, with its column header and the `..` row at index 0 (jsdom lays
    // out no visible range, so the share rows themselves never fetch here).
    expect(pane.getListingId()).toBe(lastListingId())
    expect(getTarget().querySelector('[role="listbox"]')).toBeTruthy()
    expect(getTarget().textContent).toContain('..')
    expect(getTarget().querySelector('.server-row'), 'the hub isn’t up').toBeNull()
    // The footer counts the shares and says nothing about disk space, which no server answers.
    expect(getTarget().querySelector('[role="meter"]')).toBeNull()

    await pane.navigateToParent()
    await waitForUpdates(100)

    expect(committed.at(-1)).toBe('smb://')
    await vi.waitFor(() => {
      expect(getTarget().querySelector('.server-row.is-under-cursor')?.textContent).toContain('Testnas')
    })
    // The landing is spent once the hub has it.
    expect(getActiveTab(explorerState.getTabMgr('left')).landOnServer).toBeUndefined()
    await unmount(component)
  })
})
