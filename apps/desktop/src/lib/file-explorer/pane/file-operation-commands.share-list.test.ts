import { describe, it, expect, vi, beforeEach } from 'vitest'
import { emit } from '@tauri-apps/api/event'

// The share-list half of the `file-operation-commands` specs: every verb a server's
// share list refuses. Stubs and fixtures are shared with `file-operation-commands.test.ts`
// through `file-operation-commands.test-harness.ts`, which each factory below reaches by
// a lazy `await import`.
vi.mock('#lib/tauri-commands/index.js', async () => {
  const { spies } = await import('./file-operation-commands.test-harness')
  return {
    DEFAULT_VOLUME_ID: 'root',
    getFileAt: spies.getFileAt,
    trashRoutingForPaths: spies.trashRoutingForPaths,
    checkHomeFolderGuard: spies.checkHomeFolderGuard,
  }
})

vi.mock('#lib/ui/toast/index.js', async () => ({
  addToast: (await import('./file-operation-commands.test-harness')).spies.addToast,
}))

vi.mock('#lib/search/snapshot-store.svelte.js', async () => {
  const { spies, resolveSnapshotEntriesStub } = await import('./file-operation-commands.test-harness')
  return {
    getSnapshot: spies.getSnapshot,
    resolveSnapshotEntries: resolveSnapshotEntriesStub,
    // Pure namespace arithmetic with no store state behind it, so the mock keeps
    // the real shape rather than a spy.
    snapshotIdFromPanePath: (path: string) =>
      path.startsWith('search-results://') ? path.slice('search-results://'.length) : null,
  }
})

vi.mock('#lib/stores/volume-store.svelte.js', () => ({ getVolumes: () => [] }))

vi.mock('#lib/search/capabilities.js', () => ({
  searchResultsNotAFolderToast: () => "Search results aren't a folder. Pick a real destination.",
}))

vi.mock('#lib/file-viewer/open-viewer.js', async () => ({
  openFileViewer: (await import('./file-operation-commands.test-harness')).spies.openFileViewer,
}))

vi.mock('#lib/file-operations/mkdir/new-folder-operations.js', async () => ({
  getInitialFolderName: (await import('./file-operation-commands.test-harness')).spies.getInitialFolderName,
}))

vi.mock('#lib/file-operations/mkfile/new-file-operations.js', async () => ({
  getInitialFileName: (await import('./file-operation-commands.test-harness')).spies.getInitialFileName,
}))

// `buildTransferPropsFromSnapshot` stays real: it is the builder under test here.
vi.mock('./transfer-operations', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./transfer-operations')>()
  const { spies } = await import('./file-operation-commands.test-harness')
  return {
    ...actual,
    buildTransferPropsFromSelection: spies.buildTransferPropsFromSelection,
    buildTransferPropsFromCursor: spies.buildTransferPropsFromCursor,
  }
})

vi.mock('#lib/logging/logger.js', async () => {
  const { spies } = await import('./file-operation-commands.test-harness')
  return { getAppLogger: () => ({ error: vi.fn(), warn: spies.logWarn, info: vi.fn(), debug: spies.logDebug }) }
})

import { createFileOperationCommands } from './file-operation-commands'
import {
  spies,
  buildAccess,
  buildDialogs,
  buildPaneRef,
  volume,
  type DialogsStub,
} from './file-operation-commands.test-harness'

const { buildTransferPropsFromCursor: buildFromCursorSpy, trashRoutingForPaths: trashRoutingSpy } = spies

function create(access: ReturnType<typeof buildAccess>, dialogs: DialogsStub) {
  return createFileOperationCommands(access, dialogs as unknown as Parameters<typeof createFileOperationCommands>[1])
}

beforeEach(() => {
  vi.clearAllMocks()
  trashRoutingSpy.mockResolvedValue({ routing: 'trash', folderMayHoldOnlineOnly: false })
  spies.checkHomeFolderGuard.mockResolvedValue(null)
})

/**
 * A server's share list (`network` at `smb://<server>`) is the first `canBeSource: false`
 * kind with real rows and a listing id, so every verb that takes its rows somewhere, or
 * lands something in it, refuses with words (typed for MCP) before any dialog or write.
 */
describe("a server's share list refuses every file operation", () => {
  const SHARE_LIST = 'smb://testnas'
  const WRITE = [
    'Open a share first',
    'This list shows the server’s shares. Open one to create, rename, or delete files in it.',
  ]

  function onShareList(side: 'left' | 'right' = 'left') {
    const paneRef = buildPaneRef({
      listingId: 'lst-shares',
      selectedIndices: [],
      cursorIndex: 1,
      currentPath: SHARE_LIST,
    })
    return buildAccess({
      focusedPane: 'left',
      paneRefs: { [side]: paneRef },
      paths: { [side]: SHARE_LIST },
      volumeIds: { [side]: 'network' },
      volumes: [volume()],
    })
  }

  it('refuses F7, ⇧F4, F8, and F2 with the share-list words', async () => {
    const dialogs = buildDialogs()
    const commands = create(onShareList(), dialogs)

    await commands.openNewFolderDialog({ initiator: 'user' })
    await commands.openNewFileDialog({ initiator: 'user' })
    await commands.openDeleteDialog({ permanent: false, initiator: 'user' })
    commands.startRename()

    expect(dialogs.showAlert.mock.calls).toEqual([WRITE, WRITE, WRITE, WRITE])
    expect(dialogs.showNewFolder).not.toHaveBeenCalled()
    expect(dialogs.showNewFile).not.toHaveBeenCalled()
    expect(dialogs.showDeleteConfirmation).not.toHaveBeenCalled()
  })

  it('refuses MCP `mkdir`, `delete`, and `rename` typed, with no alert over what the person does', async () => {
    const dialogs = buildDialogs()
    const commands = create(onShareList(), dialogs)

    await commands.openNewFolderDialog({ initiator: 'aiClient', mcpRequestId: 'mkdir' })
    await commands.openDeleteDialog({ permanent: false, initiator: 'aiClient', mcpRequestId: 'delete' })
    commands.startRename({ mcpRequestId: 'rename' })

    expect(dialogs.showAlert).not.toHaveBeenCalled()
    for (const requestId of ['mkdir', 'delete', 'rename']) {
      expect(emit).toHaveBeenCalledWith(
        'mcp-response',
        expect.objectContaining({ requestId, ok: false, refusal: 'readOnly' }),
      )
    }
  })

  it('refuses F5 and F6 out of a share list, and MCP `copy` typed as a source refusal', async () => {
    const dialogs = buildDialogs()
    const commands = create(onShareList(), dialogs)

    await commands.openTransferDialog('copy', { initiator: 'user' })
    await commands.openTransferDialog('move', { initiator: 'user' })
    await commands.openTransferDialog('copy', { initiator: 'aiClient', mcpRequestId: 'copy' })

    const SOURCE =
      'A share is a place on the server, so there’s nothing here to copy. Open the share to copy files from it.'
    expect(dialogs.showAlert.mock.calls).toEqual([
      ['Open a share first', SOURCE],
      ['Open a share first', SOURCE],
    ])
    expect(emit).toHaveBeenCalledWith(
      'mcp-response',
      expect.objectContaining({ requestId: 'copy', ok: false, refusal: 'source' }),
    )
    expect(dialogs.showTransfer).not.toHaveBeenCalled()
    expect(buildFromCursorSpy).not.toHaveBeenCalled()
  })

  it('refuses F5 from the other pane INTO a share list as a destination', async () => {
    const access = onShareList('right')
    const dialogs = buildDialogs()

    await create(access, dialogs).openTransferDialog('copy', { initiator: 'user' })

    expect(dialogs.showAlert).toHaveBeenCalledWith(
      'Open a share first',
      'This list shows the server’s shares, so files can’t go here. Open a share, then copy or move into it.',
    )
    expect(dialogs.showTransfer).not.toHaveBeenCalled()
  })
})
