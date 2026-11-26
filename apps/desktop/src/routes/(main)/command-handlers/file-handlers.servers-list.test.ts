/**
 * On the Servers list (`network` at `smb://`), F7 is "Add server" and F8 is "Forget"; an
 * agent's `mkdir` / `delete` there keeps its own road and refusal. On a share list F7 and F8
 * stay refusals. Get info and its siblings say a server is no file, ❌ never hand its place
 * path to a file verb.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const h = vi.hoisted(() => ({
  path: 'smb://',
  openAddServer: vi.fn(() => Promise.resolve()),
  forgetServerInView: vi.fn(() => Promise.resolve()),
  openGetInfoOrExplain: vi.fn(() => Promise.resolve()),
  addToast: vi.fn(),
}))

vi.mock('#lib/ui/toast/index.js', () => ({ addToast: h.addToast }))
vi.mock('#lib/tauri-commands/index.js', () => ({ trackEvent: vi.fn(() => Promise.resolve()) }))
vi.mock('#lib/file-explorer/pane/focused-pane-reads.js', () => ({
  getFocusedPanePath: () => h.path,
  getFocusedPaneVolumeId: () => 'network',
}))
vi.mock('#lib/stores/volume-store.svelte.js', () => ({ getVolumes: () => [] }))
vi.mock('#lib/file-explorer/pane/get-info-open.js', () => ({ openGetInfoOrExplain: h.openGetInfoOrExplain }))
vi.mock('./servers-handlers', () => ({ openAddServer: h.openAddServer, forgetServerInView: h.forgetServerInView }))

import { fileHandlers } from './file-handlers'
import type { CommandHandlerContext } from './types'

function explorer() {
  return {
    openNewFolderDialog: vi.fn(() => Promise.resolve()),
    openDeleteDialog: vi.fn(() => Promise.resolve()),
    getFileAndPathUnderCursor: () =>
      Promise.resolve({
        path: 'sftp://test_user_1@testhost:22',
        filename: 'Testhost',
        iconId: 'network:server',
        isDirectory: true,
      }),
  }
}

function run(id: 'file.newFolder' | 'file.delete' | 'file.deletePermanently' | 'file.getInfo', args?: unknown) {
  const explorerRef = explorer()
  void fileHandlers[id]({ explorerRef, ctx: {}, dispatchArgs: args } as unknown as CommandHandlerContext)
  return explorerRef
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('F7 and F8 on the Servers list', () => {
  it('F7 opens Add server, ❌ not a New folder dialog', () => {
    h.path = 'smb://'
    const ref = run('file.newFolder')
    expect(h.openAddServer).toHaveBeenCalledWith(ref)
    expect(ref.openNewFolderDialog).not.toHaveBeenCalled()
  })

  it('F8 forgets the server under the cursor, ❌ not a delete', () => {
    h.path = 'smb://'
    const ref = run('file.delete')
    expect(h.forgetServerInView).toHaveBeenCalledWith(ref)
    expect(ref.openDeleteDialog).not.toHaveBeenCalled()
  })

  /** ❗ Forgetting a server is already for good, so ⇧F8 asks exactly what F8 asks. */
  it('⇧F8 forgets the server under the cursor too, ❌ not a delete', () => {
    h.path = 'smb://'
    const ref = run('file.deletePermanently')
    expect(h.forgetServerInView).toHaveBeenCalledWith(ref)
    expect(ref.openDeleteDialog).not.toHaveBeenCalled()
  })

  it('leaves an agent’s mkdir and delete on their own road there', () => {
    h.path = 'smb://'
    expect(run('file.newFolder', { mcpRequestId: 'r', initiator: 'aiClient' }).openNewFolderDialog).toHaveBeenCalled()
    expect(run('file.delete', { mcpRequestId: 'r', initiator: 'aiClient' }).openDeleteDialog).toHaveBeenCalled()
    expect(h.openAddServer).not.toHaveBeenCalled()
    expect(h.forgetServerInView).not.toHaveBeenCalled()
  })

  it('keeps F7 and F8 on a share list on their refusing roads', () => {
    h.path = 'smb://testnas'
    expect(run('file.newFolder').openNewFolderDialog).toHaveBeenCalled()
    expect(run('file.delete').openDeleteDialog).toHaveBeenCalled()
    expect(run('file.deletePermanently').openDeleteDialog).toHaveBeenCalled()
  })

  it('says a server is no file on Get info, ❌ never asks About for its place path', async () => {
    h.path = 'smb://'
    run('file.getInfo')
    await vi.waitFor(() => {
      expect(h.addToast).toHaveBeenCalled()
    })
    expect(h.openGetInfoOrExplain).not.toHaveBeenCalled()
  })
})
