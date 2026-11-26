/**
 * Edit and Rename on the Servers volume are "Edit server…": `file.edit`, `file.rename`, and `servers.edit` all hand
 * `editServerInView` what the pane shows (the hub row under the cursor, or the host whose share list is up), whatever
 * keys they're bound to, and keep their file-list meaning everywhere else.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const editServerInView = vi.fn((_view: unknown) => Promise.resolve())
const runServerRowAction = vi.fn((_payload: unknown) => Promise.resolve())
const openInEditorOrExplain = vi.fn(() => Promise.resolve('opened'))
let volumeId = 'root'
let focusedPath = '/Users/me'

vi.mock('#lib/file-explorer/network/servers-hub-actions.js', () => ({
  editServerInView: (view: unknown) => editServerInView(view),
}))
vi.mock('#lib/file-explorer/navigation/server-row-actions.js', () => ({
  runServerRowAction: (payload: unknown) => runServerRowAction(payload),
}))
vi.mock('#lib/file-explorer/pane/editor-open.js', () => ({ openInEditorOrExplain: () => openInEditorOrExplain() }))
vi.mock('#lib/file-explorer/pane/focused-pane-reads.js', () => ({
  getFocusedPanePath: () => focusedPath,
  getFocusedPaneVolumeId: () => volumeId,
}))
vi.mock('#lib/stores/volume-store.svelte.js', () => ({ getVolumes: () => [] }))
// The server a share-list path names, as the discovery state holds it.
vi.mock('#lib/file-explorer/network/network-store.svelte.js', () => ({
  getNetworkHosts: () => [{ id: 'h1', name: 'Testnas', port: 445, source: 'discovered' }],
  setListedAccount: vi.fn(),
}))
vi.mock('#lib/tauri-commands/index.js', () => ({ listSavedServers: () => Promise.resolve([]), trackEvent: vi.fn() }))

import { fileHandlers } from './file-handlers'
import { serversHandlers } from './servers-handlers'
import type { CommandHandlerContext } from './types'
import type { HubRow } from '#lib/file-explorer/network/servers-hub-rows.js'
import type { NetworkHost } from '#lib/file-explorer/types.js'

const hubRow = { id: 'manual-nas', kind: 'server', name: 'Testnas' } as unknown as HubRow
const host: NetworkHost = { id: 'h1', name: 'Testnas', port: 445, source: 'discovered' }

/** The pane on the hub (its cursor on `row`), or on `placesHost`'s share list: the path names the server. */
function hctx(row: HubRow | null, placesHost: NetworkHost | null = null) {
  focusedPath = placesHost ? 'smb://testnas' : volumeId === 'network' ? 'smb://' : '/Users/me'
  const explorerRef = {
    getFocusedPaneServerRow: vi.fn(() => Promise.resolve(row)),
    startRename: vi.fn(),
    getFileAndPathUnderCursor: vi.fn(() => ({ path: '/Users/me/a.txt', filename: 'a.txt' })),
  }
  return { ctx: { explorerRef, ctx: {}, dispatchArgs: undefined } as unknown as CommandHandlerContext, explorerRef }
}

/** Lets the row read (async) and the hand-over it feeds run. */
async function settled(): Promise<void> {
  await vi.waitFor(() => {
    expect(editServerInView).toHaveBeenCalled()
  })
}

type Handler = (hctx: CommandHandlerContext) => unknown
const onServersVolume: [string, Handler][] = [
  ['file.rename', fileHandlers['file.rename']],
  ['file.edit', fileHandlers['file.edit']],
  ['servers.edit', serversHandlers['servers.edit']],
]

beforeEach(() => {
  vi.clearAllMocks()
  volumeId = 'root'
  focusedPath = '/Users/me'
})

describe('on the Servers volume', () => {
  beforeEach(() => {
    volumeId = 'network'
  })

  it.each(onServersVolume)('%s edits the hub row under the cursor', async (_id, handler) => {
    const { ctx, explorerRef } = hctx(hubRow)
    void handler(ctx)
    await settled()
    expect(editServerInView).toHaveBeenCalledExactlyOnceWith({ row: hubRow, host: null })
    expect(explorerRef.startRename).not.toHaveBeenCalled()
    expect(openInEditorOrExplain).not.toHaveBeenCalled()
  })

  it.each(onServersVolume)('%s edits the host whose share list is up', async (_id, handler) => {
    const { ctx } = hctx(null, host)
    void handler(ctx)
    await settled()
    expect(editServerInView).toHaveBeenCalledExactlyOnceWith({ row: null, host })
  })

  /** "Add server…" under the cursor: `editServerInView` says to pick a server, ❌ never silence. */
  it('hands over an empty view too, and never falls back to renaming a file', async () => {
    const { ctx, explorerRef } = hctx(null)
    const rename = fileHandlers['file.rename']
    rename(ctx)
    await settled()
    expect(editServerInView).toHaveBeenCalledExactlyOnceWith({ row: null, host: null })
    expect(explorerRef.startRename).not.toHaveBeenCalled()
  })
})

describe('anywhere else', () => {
  it('file.rename renames the file under the cursor', () => {
    const { ctx, explorerRef } = hctx(null)
    const rename = fileHandlers['file.rename']
    rename(ctx)
    expect(explorerRef.startRename).toHaveBeenCalledOnce()
    expect(editServerInView).not.toHaveBeenCalled()
  })

  it('file.edit opens the file under the cursor in the editor', async () => {
    const { ctx } = hctx(null)
    await (fileHandlers['file.edit'] as Handler)(ctx)
    expect(openInEditorOrExplain).toHaveBeenCalledOnce()
    expect(editServerInView).not.toHaveBeenCalled()
  })
})
