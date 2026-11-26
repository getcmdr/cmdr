/**
 * A pane on a server's share list (`network` at `smb://<server>`) is a real listing the
 * standard views render, with its kind's own words (the Description column, "2 shares",
 * "Back to Servers"), and its `..` loads the Servers list (`smb://`) with the cursor on THAT server's
 * row, found by its path. A refused listing is the calm sign-in screen naming the server. The doors and the
 * mount are `share-list-pane.svelte.test.ts`'s; this starts the pane on its tab's path.
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
import { registerFocusHome } from '#lib/ui/focus-restore.js'
import { focusActivePane } from './active-pane-focus'
import { MountFailure } from '../network/mount-error'

const events = vi.hoisted(() => ({ recorder: null as ListenRecorder | null }))
const listDirectoryStartMock = vi.hoisted(() => vi.fn())
const findPathIndexMock = vi.hoisted(() => vi.fn())
const mountNetworkShareMock = vi.hoisted(() => vi.fn())
const cancelServerConnectMock = vi.hoisted(() => vi.fn())
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
    mountNetworkShare: mountNetworkShareMock,
    cancelServerConnect: cancelServerConnectMock,
    newServerAttemptId: () => 'server-connect-test-attempt',
    cancelListing: vi.fn().mockResolvedValue(undefined),
    listDirectoryEnd: vi.fn().mockResolvedValue(undefined),
    onListingGone: vi.fn(() => () => {}),
    keepListingsAlive: vi.fn().mockResolvedValue([]),
    getFileRange: vi.fn((_id: string, start: number, count: number) =>
      Promise.resolve(SHARES.slice(start, start + count) as unknown as FileEntry[]),
    ),
    getFileAt: vi.fn((_id: string, index: number) => Promise.resolve(SHARES[index] ?? null)),
    findFileIndex: vi.fn().mockResolvedValue(null),
    findPathIndex: findPathIndexMock,
    getTotalCount: vi.fn().mockResolvedValue(SHARES.length),
    // What the backend counts for the fixture's two shares: folders, no files, no bytes.
    getListingStats: vi.fn().mockResolvedValue({
      totalFiles: 0,
      totalDirs: 2,
      totalSize: 0,
      totalPhysicalSize: 0,
      selectedFiles: null,
      selectedDirs: null,
      selectedSize: null,
      selectedPhysicalSize: null,
    }),
    // The hidden-files resync answers the listing's own count: the shares hide no rows.
    setListingIncludeHidden: vi
      .fn()
      .mockResolvedValue({ sequence: 0, totalCount: SHARES.length, newCursorIndex: null, newSelectedIndices: null }),
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

  it('renders the shares as a listing, and `..` loads the Servers list with the cursor on that server', async () => {
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
        getActiveTab(explorerState.getTabMgr('left')).path = path
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
    // The path bar carries the server's account door (no listing said who yet, so it offers Sign in as…).
    const account = getTarget().querySelector('[data-pane-chrome] button.share-list-account')
    expect(account?.textContent.trim()).toBe('Sign in as…')
    expect(account?.getAttribute('aria-label')).toBe('Account on Testnas: Sign in as…')
    // The footer counts the shares and says nothing about disk space, which no server answers.
    expect(getTarget().querySelector('[role="meter"]')).toBeNull()
    await vi.waitFor(() => {
      expect(getTarget().querySelector('.selection-info')?.textContent).toContain('No selection, 2 shares.')
    })
    // The Ext column is the shares' Description (jsdom renders no rows, so `..`'s own name
    // is `file-row-a11y.test.ts`'s).
    expect(getTarget().querySelector('.full-list-header')?.textContent ?? getTarget().textContent).toContain(
      'Description',
    )

    // ❗ `smb://`, ❌ never the TS grammar's `parentOf('smb://testnas')` (`smb:/`). The Servers
    // row reads "Testnas" while its path is the share list's, so the landing goes BY PATH.
    findPathIndexMock.mockResolvedValue(1)
    await expect(pane.navigateToParent()).resolves.toBe(true)
    await waitForUpdates(50)
    expect(listDirectoryStartMock.mock.calls.at(-1)?.[1]).toBe('smb://')
    events.recorder?.fireListingEvent('listing-complete', {
      listingId: lastListingId(),
      totalCount: 3,
      volumeRoot: 'smb://',
    })
    await waitForUpdates(100)

    expect(committed.at(-1)).toBe('smb://')
    expect(findPathIndexMock).toHaveBeenCalledWith(lastListingId(), 'smb://testnas', true)
    // The Servers list is the volume's root: no `..`, so the row's index is the pane's.
    expect(pane.getCursorIndex()).toBe(1)
    await unmount(component)
  })

  it('shows a refused listing as the calm sign-in screen, naming the server by its label', async () => {
    net.hosts = [ATTIC, TESTNAS]
    listDirectoryStartMock.mockImplementation((_v, _p, _h, _sb, _so, listingId: string) =>
      Promise.resolve({ listingId, status: { status: 'loading' } }),
    )
    explorerState.setTabMgr('left', createTabManager(createInitialTabState('smb://testnas', 'network')))
    const component = mount(FilePane, {
      target: getTarget(),
      props: {
        paneId: 'left',
        initialPath: 'smb://testnas',
        volumeId: 'network',
        volumePath: 'smb://',
        isFocused: true,
        showHiddenFiles: true,
        viewMode: 'full',
      },
    })
    await waitForUpdates()
    events.recorder?.fireListingEvent('listing-error', {
      listingId: lastListingId(),
      message: 'sign in',
      error: {
        category: 'needs_action',
        reason: { reason: 'shareListSignIn', refusal: { type: 'auth_required', message: '' }, server: 'Testnas' },
        provider: null,
        actionKind: 'sign_in_to_server',
        retryHint: false,
        rawDetail: '',
      },
    })
    await vi.waitFor(() => {
      expect(getTarget().querySelector('.error-pane')).toBeTruthy()
    })
    const errorPane = getTarget().querySelector('.error-pane')
    // ❗ No assertive alert: the sheet opens over it, and VoiceOver would read both.
    expect(errorPane?.getAttribute('role')).toBeNull()
    expect(errorPane?.querySelector('.folder-path')?.textContent).toBe('Testnas')
    expect([...(errorPane?.querySelectorAll('button') ?? [])].map((b) => b.textContent.trim())).toContain('Sign in')
    // A sign-in class offers no Try again: without an account, the server answers the same.
    expect(errorPane?.textContent).not.toContain('Try again')
    await unmount(component)
  })

  /**
   * Keyboard only, the way `DualPaneExplorer` wires it: the explorer container is the focus
   * home (its refocus is `focusActivePane`) and forwards the keys that bubble to it to the pane.
   * Keys go to `document.activeElement`, so a focus left on `<body>` reaches no pane, as in the app.
   *
   * ❗ A component pin, ❌ not an E2E one: the E2E guest share is pre-mounted, so its mount answers
   * at once and there's no wait to Escape out of.
   */
  it('Enter on a share starts the mount, Escape cancels it, and the arrows move the cursor again', async () => {
    net.hosts = [ATTIC, TESTNAS]
    listDirectoryStartMock.mockImplementation((_v, _p, _h, _sb, _so, listingId: string) =>
      Promise.resolve({ listingId, status: { status: 'loading' } }),
    )
    let refuseMount: (error: unknown) => void = () => {}
    mountNetworkShareMock.mockImplementation(
      () =>
        new Promise((_resolve, reject) => {
          refuseMount = reject
        }),
    )
    // The backend answers a cancel by ending the mount as `cancelled`.
    cancelServerConnectMock.mockImplementation(() => {
      refuseMount(new MountFailure({ type: 'cancelled', share: 'public' }))
      return Promise.resolve(true)
    })
    explorerState.setTabMgr('left', createTabManager(createInitialTabState('smb://testnas', 'network')))

    const explorer = getTarget()
    explorer.tabIndex = -1
    const component = mount(FilePane, {
      target: explorer,
      props: {
        paneId: 'left',
        initialPath: 'smb://testnas',
        volumeId: 'network',
        volumePath: 'smb://',
        isFocused: true,
        showHiddenFiles: true,
        viewMode: 'full',
      },
    })
    const pane = component as unknown as FilePaneAPI
    const unregisterHome = registerFocusHome({
      root: () => explorer,
      refocus: () => {
        focusActivePane({ getPaneRef: () => pane, getFocusedPane: () => 'left', getContainerElement: () => explorer })
      },
    })
    const forward = (e: KeyboardEvent) => {
      pane.handleKeyDown(e)
    }
    explorer.addEventListener('keydown', forward)
    const press = async (key: string) => {
      const target = document.activeElement ?? document.body
      target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
      await waitForUpdates()
    }
    const listbox = () => explorer.querySelector<HTMLElement>('[role="listbox"]')

    await waitForUpdates()
    events.recorder?.fireListingEvent('listing-complete', {
      listingId: lastListingId(),
      totalCount: SHARES.length,
      volumeRoot: 'smb://',
    })
    await waitForUpdates(100)
    expect(pane.focusList()).toBe(true)
    expect(document.activeElement).toBe(listbox())

    await vi.waitFor(() => {
      expect(pane.getEffectiveTotalCount(), 'the two shares and `..`').toBe(SHARES.length + 1)
    })
    // `..` is row 0, so ↓ lands on `public`, and Enter mounts it.
    await press('ArrowDown')
    expect(pane.getCursorIndex()).toBe(1)
    await press('Enter')
    await vi.waitFor(() => {
      expect(mountNetworkShareMock).toHaveBeenCalledOnce()
    })
    expect(mountNetworkShareMock.mock.calls[0][1]).toBe('public')
    // The wait replaced the list, and focus went home to the explorer rather than `<body>`.
    expect(explorer.querySelector('.remote-connect')).toBeTruthy()
    expect(listbox()).toBeNull()
    expect(document.activeElement).toBe(explorer)

    await press('Escape')
    expect(cancelServerConnectMock).toHaveBeenCalledWith('server-connect-test-attempt')
    await vi.waitFor(() => {
      expect(listbox()).toBeTruthy()
    })
    expect(explorer.querySelector('.remote-connect')).toBeNull()
    // Focus came back to the share list, so the arrows reach it.
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(listbox())
    })
    const before = pane.getCursorIndex()
    await press(before === 0 ? 'ArrowDown' : 'ArrowUp')
    expect(pane.getCursorIndex()).toBe(before === 0 ? 1 : before - 1)

    explorer.removeEventListener('keydown', forward)
    unregisterHome()
    await unmount(component)
  })
})
