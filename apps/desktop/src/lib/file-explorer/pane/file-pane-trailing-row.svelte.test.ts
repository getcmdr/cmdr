/**
 * A pane whose kind asks for a trailing action row (`trailing-row-pane.ts`), on a STUB kind: a
 * plain local folder whose kind words are mocked to end in "Make a thing…". Pins the wiring
 * `FilePane` owns, the parts no pure module sees: the cursor keys reach the row, Enter and MCP
 * `open_under_cursor` on it run its command through the pane's `onCommand`, no selection
 * gesture takes it, and MCP `move_cursor` finds it by name and index. (jsdom lays out no rows
 * here; the views' own drawing is `../views/trailing-row.views.test.ts`'s.)
 */
import { describe, it, expect, vi } from 'vitest'
import { mount, unmount } from 'svelte'
import FilePane from './FilePane.svelte'
import { waitForUpdates, useMountTarget, type ListenRecorder } from './integration-test-utils'
import type { FilePaneAPI } from './types'
import type { FileEntry } from '../types'
import { explorerState } from './explorer-state.svelte'
import { createInitialTabState } from './tab-operations'
import { createTabManager } from '../tabs/tab-state-manager.svelte'

const events = vi.hoisted(() => ({ recorder: null as ListenRecorder | null }))
const listDirectoryStartMock = vi.hoisted(() => vi.fn())

/** The stub folder's three rows. */
const ROWS = vi.hoisted(() =>
  ['alpha', 'beta', 'gamma'].map((name) => ({
    name,
    path: `/stub/${name}`,
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
  })),
)

vi.mock('./list-kind-words', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./list-kind-words')>()),
  trailingRowFor: () => ({ label: 'Make a thing…', commandId: 'servers.connect', mcpName: '+ Make a thing…' }),
}))

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
      Promise.resolve(ROWS.slice(start, start + count) as unknown as FileEntry[]),
    ),
    getFileAt: vi.fn((_id: string, index: number) => Promise.resolve(ROWS[index] ?? null)),
    findFileIndex: vi.fn().mockResolvedValue(null),
    getTotalCount: vi.fn().mockResolvedValue(ROWS.length),
    getListingStats: vi.fn().mockResolvedValue({
      totalFiles: 0,
      totalDirs: 3,
      totalSize: 0,
      totalPhysicalSize: 0,
      selectedFiles: null,
      selectedDirs: null,
      selectedSize: null,
      selectedPhysicalSize: null,
    }),
    setListingIncludeHidden: vi
      .fn()
      .mockResolvedValue({ sequence: 0, totalCount: ROWS.length, newCursorIndex: null, newSelectedIndices: null }),
    getSyncStatus: vi.fn().mockResolvedValue({ data: {}, timedOut: false }),
    onMediaEnrichProgress: vi.fn().mockResolvedValue(() => {}),
    onMediaEnrichTerminal: vi.fn().mockResolvedValue(() => {}),
    listen: recorder.listen,
    ...recorder.listingWrappers,
    showFileContextMenu: vi.fn().mockResolvedValue(undefined),
    updateMenuContext: vi.fn().mockResolvedValue(undefined),
    updateServicesSelection: vi.fn().mockResolvedValue(undefined),
    updateSelectSameKindMenu: vi.fn().mockResolvedValue(undefined),
    updateLeftPaneState: vi.fn().mockResolvedValue(undefined),
    updateRightPaneState: vi.fn().mockResolvedValue(undefined),
    listVolumes: vi.fn().mockResolvedValue({ data: [], timedOut: false }),
    resolvePathVolume: vi.fn().mockResolvedValue({ volume: null, timedOut: false }),
    getDefaultVolumeId: vi.fn().mockResolvedValue('root'),
    getVolumeSpace: vi.fn().mockResolvedValue({ data: null, timedOut: false }),
    refreshListing: vi.fn().mockResolvedValue({ data: null, timedOut: false }),
    getIcons: vi.fn().mockResolvedValue({ data: {}, timedOut: false }),
    refreshDirectoryIcons: vi.fn().mockResolvedValue({ data: {}, timedOut: false }),
    DEFAULT_VOLUME_ID: 'root',
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
  getMediaIndexEnabled: vi.fn().mockReturnValue(false),
  getMediaIndexShowFileStatusIcons: vi.fn().mockReturnValue(false),
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

function key(k: string, init: KeyboardEventInit = {}): KeyboardEvent {
  return new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init })
}

describe('FilePane with a trailing action row (stub kind)', () => {
  const { getTarget } = useMountTarget()

  async function mountStubPane() {
    listDirectoryStartMock.mockImplementation((_v, _p, _h, _sb, _so, listingId: string) =>
      Promise.resolve({ listingId, status: { status: 'loading' } }),
    )
    explorerState.setTabMgr('left', createTabManager(createInitialTabState('/stub', 'root')))
    const onCommand = vi.fn()
    const component = mount(FilePane, {
      target: getTarget(),
      props: {
        paneId: 'left',
        initialPath: '/stub',
        volumeId: 'root',
        volumePath: '/stub',
        isFocused: true,
        showHiddenFiles: true,
        viewMode: 'full',
        onCommand,
      },
    })
    const pane = component as unknown as FilePaneAPI
    await waitForUpdates()
    const calls = listDirectoryStartMock.mock.calls
    events.recorder?.fireListingEvent('listing-complete', {
      listingId: calls[calls.length - 1][5] as string,
      totalCount: ROWS.length,
      volumeRoot: '/stub',
    })
    await waitForUpdates(100)
    return { component, pane, onCommand }
  }

  it('the cursor keys reach it last, and Enter on it runs its command', async () => {
    const { component, pane, onCommand } = await mountStubPane()
    // At the volume root: no `..`, three entries at 0–2, the trailing row at 3.
    expect(pane.getEffectiveTotalCount()).toBe(3)
    expect(pane.getCursorRowCount()).toBe(4)

    pane.handleKeyDown(key('End'))
    expect(pane.getCursorIndex()).toBe(3)
    pane.handleKeyDown(key('ArrowDown'))
    expect(pane.getCursorIndex()).toBe(3)
    pane.handleKeyDown(key('Enter'))
    expect(onCommand).toHaveBeenCalledWith('servers.connect')
    await unmount(component)
  })

  it('no selection gesture takes it: Space, Insert, ⇧↓ onto it, ⌘A', async () => {
    const { component, pane } = await mountStubPane()
    await pane.setCursorIndex(2)
    pane.handleKeyDown(key('ArrowDown', { shiftKey: true }))
    expect(pane.getCursorIndex()).toBe(3)
    expect(pane.getSelectedIndices()).toEqual([2])
    pane.toggleSelectionAtCursor()
    expect(pane.getSelectedIndices()).toEqual([2])
    pane.toggleSelectionAndMoveDownAtCursor()
    expect(pane.getSelectedIndices()).toEqual([2])
    expect(pane.getCursorIndex()).toBe(3)
    await pane.selectAll()
    expect(pane.getSelectedIndices()).toEqual([0, 1, 2])
    await unmount(component)
  })

  it('MCP finds it by name and index, and `open_under_cursor` runs it', async () => {
    const { component, pane, onCommand } = await mountStubPane()
    expect(pane.trailingRowIndexNamed('+ Make a thing…')).toBe(3)
    expect(pane.trailingRowIndexNamed('alpha')).toBeNull()
    await pane.setCursorIndex(3)
    await pane.openCursorItem()
    expect(onCommand).toHaveBeenCalledWith('servers.connect')
    // A person's consent names a path; the row has none, so a tool asking under one is told it changed.
    expect(await pane.openCursorItemForTool({ expectedPath: '/stub/gamma' })).toBe('changedWhileAsking')
    expect(await pane.openCursorItemForTool({ mayLaunch: false })).toBe('opened')
    expect(onCommand).toHaveBeenCalledTimes(2)
    await unmount(component)
  })
})
