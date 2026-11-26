/**
 * A pane's side of a server's share list (`share-list-pane.svelte.ts`): Enter on a share
 * mounts it (with no credential of the frontend's own), a door's arrival lands once the
 * list does, and a sign-in refusal asks for an account only on a share list the person
 * opened, once.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushSync } from 'svelte'
import type { FileEntry, FriendlyError, NetworkHost } from '../types'

const h = vi.hoisted(() => ({
  mountNetworkShare: vi.fn(),
  fetchShares: vi.fn(() => Promise.resolve()),
  findFileIndex: vi.fn(),
  getFileAt: vi.fn(),
  askToSignInToList: vi.fn(),
  showShareParentHintOnce: vi.fn(),
  addToast: vi.fn(() => 'id'),
  hosts: [] as NetworkHost[],
}))

vi.mock('#lib/tauri-commands/index.js', () => ({
  mountNetworkShare: h.mountNetworkShare,
  findFileIndex: h.findFileIndex,
  getFileAt: h.getFileAt,
  newServerAttemptId: () => 'attempt-1',
  cancelServerConnect: vi.fn(() => Promise.resolve(true)),
  resolvePathVolume: vi.fn(() => Promise.resolve({ volume: null })),
  saveSmbCredentials: vi.fn(() => Promise.resolve()),
  setSmbAccountPreference: vi.fn(() => Promise.resolve(true)),
}))
vi.mock('../network/network-store.svelte', () => ({
  getNetworkHosts: () => h.hosts,
  setListedAccount: vi.fn(),
  fetchShares: h.fetchShares,
}))
vi.mock('../network/share-list-sign-in', () => ({ askToSignInToList: h.askToSignInToList }))
vi.mock('../network/share-parent-hint', () => ({ showShareParentHintOnce: h.showShareParentHintOnce }))
vi.mock('#lib/ui/toast/index.js', () => ({ addToast: h.addToast }))
vi.mock('#lib/settings/network-settings.js', () => ({ getMountTimeoutMs: () => 15000 }))
vi.mock('#lib/servers/sign-in-sheet-state.svelte.js', () => ({ openSignInSheet: vi.fn() }))

import { createShareListPane, type ShareListPane } from './share-list-pane.svelte'
import { dropShareListArrivalOff, queueShareListArrival, shareListArrivalFor } from './share-list-arrival.svelte'

const testnas: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'testnas.local',
  ipAddress: '192.168.50.10',
  port: 445,
  source: 'discovered',
}

function shareRow(name: string): FileEntry {
  return {
    name,
    path: `smb://testnas/${name}`,
    isDirectory: true,
    isSymlink: false,
    permissions: 0,
    owner: '',
    group: '',
    iconId: 'dir',
    shareMeta: { description: null, saved: null },
  }
}

const signInRefusal: FriendlyError = {
  category: 'needs_action',
  title: 'Sign in to Testnas',
  explanation: '',
  suggestion: '',
  rawDetail: '',
  retryHint: false,
  actionKind: 'sign_in_to_server',
  listingReason: 'shareListSignIn',
  shareListRefusal: { type: 'auth_required', message: '' },
}

let dispose: (() => void) | undefined
afterEach(() => {
  dispose?.()
  dispose = undefined
})

beforeEach(() => {
  vi.clearAllMocks()
  h.hosts = [testnas]
  dropShareListArrivalOff('left', 'nowhere')
  h.mountNetworkShare.mockReturnValue(new Promise(() => {}))
  h.findFileIndex.mockResolvedValue(0)
  h.getFileAt.mockResolvedValue(shareRow('public'))
})

function create(initial: { volumeId?: string; path?: string } = {}) {
  const state = $state({
    volumeId: initial.volumeId ?? 'network',
    path: initial.path ?? 'smb://testnas',
    friendly: null as FriendlyError | null,
    failed: false,
  })
  const deps = {
    moveCursorTo: vi.fn(),
    relist: vi.fn(),
    focusSignIn: vi.fn(),
    onVolumeChange: vi.fn(),
    syncMcp: vi.fn(),
  }
  let pane!: ShareListPane
  dispose = $effect.root(() => {
    pane = createShareListPane({
      getPaneId: () => 'left',
      getVolumeId: () => state.volumeId,
      getCurrentPath: () => state.path,
      getListingId: () => 'listing-1',
      getIncludeHidden: () => false,
      getFriendlyError: () => state.friendly,
      getListingFailed: () => state.failed,
      ...deps,
    })
  })
  flushSync()
  return { pane, state, ...deps }
}

describe('Enter on a share', () => {
  it('mounts it, with no credential of the frontend’s own (the backend signs in)', () => {
    const { pane } = create()
    expect(pane.openShare(shareRow('public'))).toBe(true)
    const [server, share, username, password] = h.mountNetworkShare.mock.calls[0] as unknown[]
    expect([server, share, username, password]).toEqual(['192.168.50.10', 'public', null, null])
  })

  it('starts nothing while a mount already runs, and leaves `..` and other panes’ rows alone', () => {
    const { pane } = create()
    pane.openShare(shareRow('public'))
    flushSync()
    expect(pane.openShare(shareRow('media')), 'the wait owns the row').toBe(true)
    expect(h.mountNetworkShare).toHaveBeenCalledOnce()

    expect(pane.openShare({ ...shareRow('..'), name: '..' })).toBe(false)
    expect(create({ volumeId: 'root', path: '/Users/test_user_1' }).pane.openShare(shareRow('docs'))).toBe(false)
  })
})

describe('a door’s arrival', () => {
  it('opens the server’s share list from a door, queuing what to do once it lands', () => {
    const { pane, onVolumeChange } = create({ path: 'smb://' })
    pane.openServer(testnas, { mount: 'public' })
    expect(onVolumeChange).toHaveBeenCalledWith({
      volumeId: 'network',
      volumePath: 'smb://',
      targetPath: 'smb://testnas',
      exact: true,
    })
    expect(shareListArrivalFor('left')).toMatchObject({ path: 'smb://testnas', origin: 'smb://', mount: 'public' })
  })

  it('mounts the queued share once the list lands, and once only', async () => {
    const { pane } = create()
    queueShareListArrival('left', { path: 'smb://testnas', origin: 'smb://', mount: 'public' })
    pane.landed('smb://testnas')
    await vi.waitFor(() => {
      expect(h.mountNetworkShare).toHaveBeenCalledOnce()
    })
    pane.landed('smb://testnas')
    await Promise.resolve()
    expect(h.mountNetworkShare).toHaveBeenCalledOnce()
  })

  it('mounts it when the listing lands under Rust’s spelling of the same share list (a trailing `/`)', async () => {
    // Regression anchor: the landed path was compared byte-for-byte with the pane's, so
    // "Copy path between panes" spent the queued share and mounted nothing.
    const { pane } = create()
    queueShareListArrival('left', { path: 'smb://testnas', origin: '/Users/test_user_1', mount: 'public' })
    pane.landed('smb://testnas/')
    await vi.waitFor(() => {
      expect(h.mountNetworkShare).toHaveBeenCalledOnce()
    })
  })

  it('refreshes the hub’s record of the server’s shares from the cache the listing filled', () => {
    const { pane } = create()
    pane.landed('smb://testnas')
    expect(h.fetchShares).toHaveBeenCalledWith(testnas)
  })

  it('says the share wasn’t found on a list that lacks it, and mounts nothing', async () => {
    h.findFileIndex.mockResolvedValue(null)
    const { pane } = create()
    queueShareListArrival('left', { path: 'smb://testnas', origin: 'smb://', mount: 'missing' })
    pane.landed('smb://testnas')
    await vi.waitFor(() => {
      expect(h.addToast).toHaveBeenCalledWith(expect.stringContaining('missing'), { level: 'warn' })
    })
    expect(h.mountNetworkShare).not.toHaveBeenCalled()
  })

  it('keeps the share through an error screen, for the re-list a sign-in runs', async () => {
    const { pane, state } = create()
    queueShareListArrival('left', { path: 'smb://testnas', origin: 'smb://', mount: 'public' })
    state.failed = true
    pane.landed('smb://testnas')
    await Promise.resolve()
    expect(shareListArrivalFor('left')).not.toBeNull()
    expect(h.mountNetworkShare).not.toHaveBeenCalled()
  })

  it('puts the cursor on the share `..` came out of (`..` is row 0), and explains where it went', async () => {
    h.findFileIndex.mockResolvedValue(2)
    const { pane, moveCursorTo } = create()
    const land = { volumeId: 'smb-testnas-media', hostId: 'testnas-id', shareName: 'media', serverLabel: 'Testnas' }
    queueShareListArrival('left', { path: 'smb://testnas', origin: '/Volumes/media', land })
    pane.landed('smb://testnas')
    await vi.waitFor(() => {
      expect(moveCursorTo).toHaveBeenCalledWith(3)
    })
    expect(h.showShareParentHintOnce).toHaveBeenCalledWith(land)
    expect(h.mountNetworkShare).not.toHaveBeenCalled()
  })
})

describe('a sign-in refusal', () => {
  // The in-pane sign-in state is the honest "you're not signed in" place: Cancel leaves the
  // pane on it, with focus on its Sign in (Enter asks again, ⌘[ goes back), ❌ not on Servers.
  it('asks once on a share list the person opened, and a cancel stays on its Sign in', async () => {
    h.askToSignInToList.mockResolvedValue({ kind: 'cancelled' })
    const { state, focusSignIn, onVolumeChange, relist } = create()
    queueShareListArrival('left', { path: 'smb://testnas', origin: 'smb://' })
    state.friendly = signInRefusal
    flushSync()
    await vi.waitFor(() => {
      expect(focusSignIn).toHaveBeenCalledOnce()
    })
    expect(onVolumeChange).not.toHaveBeenCalled()
    expect(relist).not.toHaveBeenCalled()
    expect(h.askToSignInToList).toHaveBeenCalledExactlyOnceWith('smb://testnas', { type: 'auth_required', message: '' })

    // A repaint of the same refusal asks nothing.
    state.friendly = { ...signInRefusal }
    flushSync()
    expect(h.askToSignInToList).toHaveBeenCalledOnce()
  })

  it('re-lists once the sheet listed the server', async () => {
    h.askToSignInToList.mockResolvedValue({
      kind: 'listed',
      result: { shares: [], authMode: 'creds_required', fromCache: false },
    })
    const { state, relist } = create()
    queueShareListArrival('left', { path: 'smb://testnas', origin: 'smb://' })
    state.friendly = signInRefusal
    flushSync()
    await vi.waitFor(() => {
      expect(relist).toHaveBeenCalledOnce()
    })
  })

  it('asks nothing on a share list no door opened (a tab coming back into view)', () => {
    const { state } = create()
    state.friendly = signInRefusal
    flushSync()
    expect(h.askToSignInToList).not.toHaveBeenCalled()
  })

  it('asks again on a listing the pane was asked to run (Retry, an account door)', async () => {
    h.askToSignInToList.mockResolvedValue(null)
    const { pane, state } = create()
    pane.noteListingStarted('smb://testnas')
    state.friendly = signInRefusal
    flushSync()
    await vi.waitFor(() => {
      expect(h.askToSignInToList).toHaveBeenCalledOnce()
    })
    state.friendly = null
    flushSync()
    pane.noteListingStarted('smb://testnas')
    state.friendly = { ...signInRefusal }
    flushSync()
    await vi.waitFor(() => {
      expect(h.askToSignInToList).toHaveBeenCalledTimes(2)
    })
  })

  it('names the server by its friendly label for the error screen', () => {
    expect(create().pane.label).toBe('Testnas')
    expect(create({ path: 'smb://' }).pane.label).toBeUndefined()
  })
})
