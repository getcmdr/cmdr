/**
 * Rename on the Servers volume is "Edit server…", except MCP `rename` on a server's
 * share list: it names a ROW, so opening Edit server would answer a question the agent
 * never asked. It goes to the pane's rename, whose share-list refusal answers it typed.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const h = vi.hoisted(() => ({ path: 'smb://testnas', editServerInView: vi.fn(() => Promise.resolve()) }))

vi.mock('#lib/ui/toast/index.js', () => ({ addToast: vi.fn() }))
vi.mock('#lib/tauri-commands/index.js', () => ({ trackEvent: vi.fn(() => Promise.resolve()) }))
vi.mock('#lib/file-explorer/pane/focused-pane-reads.js', () => ({
  getFocusedPanePath: () => h.path,
  getFocusedPaneVolumeId: () => 'network',
}))
vi.mock('#lib/stores/volume-store.svelte.js', () => ({ getVolumes: () => [] }))
vi.mock('#lib/file-explorer/network/servers-hub-actions.js', () => ({ editServerInView: h.editServerInView }))

import { fileHandlers } from './file-handlers'
import type { CommandHandlerContext } from './types'

function dispatch(args: unknown) {
  const explorerRef = {
    startRename: vi.fn(),
    getFocusedPaneHubRow: () => null,
    getFocusedPaneNetworkHost: () => null,
  }
  fileHandlers['file.rename']({ explorerRef, ctx: {}, dispatchArgs: args } as unknown as CommandHandlerContext)
  return explorerRef
}

beforeEach(() => {
  h.editServerInView.mockClear()
})

describe('file.rename on the Servers volume', () => {
  it('hands MCP `rename` on a share list to the pane, which refuses it typed', () => {
    h.path = 'smb://testnas'
    const args = { initialName: 'public', expectedName: 'public', mcpRequestId: 'r-1', initiator: 'aiClient' }

    const explorer = dispatch(args)

    expect(explorer.startRename).toHaveBeenCalledWith(args)
    expect(h.editServerInView).not.toHaveBeenCalled()
  })

  it('keeps F2 on a share list, and any rename on the hub, as Edit server', () => {
    h.path = 'smb://testnas'
    expect(dispatch(undefined).startRename).not.toHaveBeenCalled()
    h.path = 'smb://'
    expect(dispatch({ mcpRequestId: 'r-2', initiator: 'aiClient' }).startRename).not.toHaveBeenCalled()
    expect(h.editServerInView).toHaveBeenCalledTimes(2)
  })
})
