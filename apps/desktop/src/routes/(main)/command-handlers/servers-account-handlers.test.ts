/**
 * The palette's door to an SMB server's account: `servers.signInAs`, `servers.browseAsGuest`,
 * and the SMB arms of `servers.forgetSecret` and `servers.disconnect`. Each acts on the hub's
 * cursor host row, else the server whose share list the pane shows, else nothing, and runs
 * the function the row menu runs, so the doors can't drift.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const runServerAccountAction = vi.fn((_action: string, _server: unknown) => Promise.resolve({ listed: false }))
const disconnectSmbHost = vi.fn((_host: unknown, _label: string) => Promise.resolve())
const runServerRowAction = vi.fn((_payload: unknown) => Promise.resolve())
let volumeId = 'network'
let focusedPath = 'smb://'

vi.mock('#lib/file-explorer/network/share-list-sign-in.js', () => ({
  runServerAccountAction: (action: string, server: unknown) => runServerAccountAction(action, server),
}))
vi.mock('#lib/file-explorer/network/servers-hub-actions.js', () => ({
  disconnectSmbHost: (host: unknown, label: string) => disconnectSmbHost(host, label),
  editServerInView: vi.fn(() => Promise.resolve()),
}))
vi.mock('#lib/file-explorer/navigation/server-row-actions.js', async (importOriginal) => ({
  ...(await importOriginal<typeof import('#lib/file-explorer/navigation/server-row-actions.js')>()),
  runServerRowAction: (payload: unknown) => runServerRowAction(payload),
}))
vi.mock('#lib/file-explorer/pane/focused-pane-reads.js', () => ({
  getFocusedPanePath: () => focusedPath,
  getFocusedPaneVolumeId: () => volumeId,
}))
vi.mock('#lib/stores/volume-store.svelte.js', () => ({ getVolumes: () => [] }))
// The server a share-list path names, as the discovery state holds it.
vi.mock('#lib/file-explorer/network/network-store.svelte.js', () => ({
  getNetworkHosts: () => [
    { id: 'testnas-id', name: 'Testnas', hostname: 'testnas.local', port: 445, source: 'discovered' },
  ],
  setListedAccount: vi.fn(),
}))
vi.mock('#lib/tauri-commands/index.js', () => ({ listSavedServers: () => Promise.resolve([]) }))

import { serversHandlers } from './servers-handlers'
import type { CommandHandlerContext } from './types'
import type { HubRow } from '#lib/file-explorer/network/servers-hub-rows.js'
import type { NetworkHost } from '#lib/file-explorer/types.js'

const nas: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'testnas.local',
  port: 445,
  source: 'discovered',
}
const hostRow = {
  id: 'manual-testnas',
  kind: 'server',
  protocol: 'smb',
  name: 'My NAS',
  address: 'testnas.local',
  volumeId: null,
  host: nas,
} as unknown as HubRow

/** The pane on the hub (its cursor on `view.row`), or on `view.shareListHost`'s share list: the path names the server. */
function hctx(view: { row?: HubRow | null; shareListHost?: NetworkHost | null }) {
  focusedPath = view.shareListHost ? 'smb://testnas' : 'smb://'
  const explorerRef = {
    getFocusedPaneHubRow: vi.fn(() => view.row ?? null),
    openSmbHandOffInFocusedPane: vi.fn(),
  }
  return { ctx: { explorerRef, ctx: {}, dispatchArgs: undefined } as unknown as CommandHandlerContext, explorerRef }
}

/** The handlers run their work detached; let it land. */
async function settled(): Promise<void> {
  await vi.waitFor(() => Promise.resolve())
  await new Promise((resolve) => setTimeout(resolve, 0))
}

beforeEach(() => {
  vi.clearAllMocks()
  volumeId = 'network'
})

describe.each([
  ['servers.signInAs', 'sign-in-as'],
  ['servers.browseAsGuest', 'browse-as-guest'],
] as const)('%s', (id, action) => {
  it('acts on the hub’s cursor host row, by the name the row shows', async () => {
    const { ctx } = hctx({ row: hostRow })
    serversHandlers[id](ctx)
    await settled()
    expect(runServerAccountAction).toHaveBeenCalledExactlyOnceWith(action, { host: nas, label: 'My NAS' })
  })

  it('acts on the server whose share list the pane shows', async () => {
    const { ctx } = hctx({ shareListHost: nas })
    serversHandlers[id](ctx)
    await settled()
    expect(runServerAccountAction).toHaveBeenCalledExactlyOnceWith(action, { host: nas, label: 'Testnas' })
  })

  it('does nothing where no SMB server is in view', async () => {
    volumeId = 'root'
    const { ctx } = hctx({})
    serversHandlers[id](ctx)
    await settled()
    expect(runServerAccountAction).not.toHaveBeenCalled()
  })

  /** From a hub row, a listing that landed opens the share list, as the row menu's does. */
  it('opens the server’s share list once it listed from a hub row', async () => {
    runServerAccountAction.mockResolvedValueOnce({ listed: true })
    const { ctx, explorerRef } = hctx({ row: hostRow })
    serversHandlers[id](ctx)
    await settled()
    expect(explorerRef.openSmbHandOffInFocusedPane).toHaveBeenCalledExactlyOnceWith({ host: nas, sharePath: null })
  })

  it('leaves a share list that listed where it is: the list follows the listing', async () => {
    runServerAccountAction.mockResolvedValueOnce({ listed: true })
    const { ctx, explorerRef } = hctx({ shareListHost: nas })
    serversHandlers[id](ctx)
    await settled()
    expect(explorerRef.openSmbHandOffInFocusedPane).not.toHaveBeenCalled()
  })
})

describe('the SMB arms of the existing server commands', () => {
  it('servers.forgetSecret forgets an SMB server’s password through the account runner', async () => {
    const { ctx } = hctx({ row: hostRow })
    await serversHandlers['servers.forgetSecret'](ctx)
    expect(runServerAccountAction).toHaveBeenCalledExactlyOnceWith('forget-secret', { host: nas, label: 'My NAS' })
    expect(runServerRowAction).not.toHaveBeenCalled()
  })

  it('servers.disconnect unmounts an SMB server’s shares, on the share list too', async () => {
    const { ctx } = hctx({ shareListHost: nas })
    await serversHandlers['servers.disconnect'](ctx)
    expect(disconnectSmbHost).toHaveBeenCalledExactlyOnceWith(nas, 'Testnas')
    expect(runServerRowAction).not.toHaveBeenCalled()
  })

  it('servers.togglePin skips an SMB server: it has no switcher row to pin', async () => {
    const { ctx } = hctx({ row: hostRow })
    await serversHandlers['servers.togglePin'](ctx)
    expect(runServerRowAction).not.toHaveBeenCalled()
  })
})
