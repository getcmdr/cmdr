import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { NavigateIntent, NavigateResult } from './navigate'
import type { FilePaneAPI } from './types'
import type { NetworkHost } from '../types'

const net = vi.hoisted(() => ({ hosts: [] as NetworkHost[] }))
vi.mock('../network/network-store.svelte', () => ({ getNetworkHosts: () => net.hosts, setListedAccount: vi.fn() }))

import { createPaneMirror, type PaneMirrorDeps } from './pane-mirror'

const testnas: NetworkHost = {
  id: 'testnas-id',
  name: 'Testnas',
  hostname: 'testnas.local',
  port: 445,
  source: 'discovered',
}

function makePaneRef(overrides: Record<string, unknown> = {}) {
  return {
    getCursorEntry: vi.fn(() => null),
    refreshCursorEntry: vi.fn(() => Promise.resolve(null)),
    openShareList: vi.fn(),
    ...overrides,
  }
}

function setup(opts: {
  refs?: Record<'left' | 'right', ReturnType<typeof makePaneRef> | undefined>
  volumeIdByPane?: Record<'left' | 'right', string>
  pathByPane?: Record<'left' | 'right', string>
  focused?: 'left' | 'right'
}) {
  const navigate = vi.fn<(i: NavigateIntent) => NavigateResult>(
    () => ({ status: 'started' }) as unknown as NavigateResult,
  )
  const setFocusedPane = vi.fn()
  let focused = opts.focused ?? 'left'
  const deps: PaneMirrorDeps = {
    navigate,
    getPaneRef: (p) => opts.refs?.[p] as unknown as FilePaneAPI | undefined,
    getPaneVolumeId: (p) => opts.volumeIdByPane?.[p] ?? 'root',
    getPanePath: (p) => opts.pathByPane?.[p] ?? '/',
    getFocusedPane: () => focused,
    setFocusedPane: (p) => {
      focused = p
      setFocusedPane(p)
    },
  }
  return { mirror: createPaneMirror(deps), navigate, setFocusedPane }
}

describe('createPaneMirror', () => {
  beforeEach(() => vi.clearAllMocks())

  it('is a no-op when source and target are the same pane', () => {
    const { mirror, navigate } = setup({ refs: { left: makePaneRef(), right: makePaneRef() } })
    mirror.copyPathBetweenPanes({ source: 'left', target: 'left' })
    expect(navigate).not.toHaveBeenCalled()
  })

  it('mirrors a local listing to the target pane via a goTo/mirror navigation', () => {
    const left = makePaneRef()
    const right = makePaneRef()
    const { mirror, navigate } = setup({
      refs: { left, right },
      volumeIdByPane: { left: 'root', right: 'usb' },
      pathByPane: { left: '/a/b', right: '/x' },
      focused: 'right', // source (left) not focused → no cursor refine
    })

    mirror.copyPathBetweenPanes({ source: 'left', target: 'right' })

    expect(navigate).toHaveBeenCalledWith({
      pane: 'right',
      to: { goTo: { volumeId: 'root', path: '/a/b' } },
      source: 'mirror',
    })
  })

  it('refines to the folder under the cursor when the source pane is focused', () => {
    const left = makePaneRef({
      getCursorEntry: vi.fn(() => ({ isDirectory: true, name: 'sub', path: '/a/b/sub' }) as never),
    })
    const right = makePaneRef()
    const { mirror, navigate } = setup({
      refs: { left, right },
      volumeIdByPane: { left: 'root', right: 'usb' },
      pathByPane: { left: '/a/b', right: '/x' },
      focused: 'left',
    })

    mirror.copyPathBetweenPanes({ source: 'left', target: 'right' })

    expect(navigate).toHaveBeenCalledWith({
      pane: 'right',
      to: { goTo: { volumeId: 'root', path: '/a/b/sub' } },
      source: 'mirror',
    })
  })

  it('copies the pane location exactly, cursor or not, with followCursor: false (Clone pane)', () => {
    const getCursorEntry = vi.fn(() => ({ isDirectory: true, name: 'sub', path: '/a/b/sub' }) as never)
    const left = makePaneRef({ getCursorEntry })
    const right = makePaneRef()
    const { mirror, navigate } = setup({
      refs: { left, right },
      volumeIdByPane: { left: 'root', right: 'usb' },
      pathByPane: { left: '/a/b', right: '/x' },
      focused: 'left',
    })

    mirror.copyPathBetweenPanes({ source: 'left', target: 'right', followCursor: false })

    expect(navigate).toHaveBeenCalledWith({
      pane: 'right',
      to: { goTo: { volumeId: 'root', path: '/a/b' } },
      source: 'mirror',
    })
    expect(getCursorEntry).not.toHaveBeenCalled()
  })

  it('skips the redundant navigation when the target already shows the same volume + path', () => {
    const { mirror, navigate, setFocusedPane } = setup({
      refs: { left: makePaneRef(), right: makePaneRef() },
      volumeIdByPane: { left: 'root', right: 'root' },
      pathByPane: { left: '/same', right: '/same' },
      focused: 'right',
    })

    mirror.copyPathBetweenPanes({ source: 'left', target: 'right' })

    expect(navigate).not.toHaveBeenCalled()
    // Focus was already on 'right' (the original), so no restore write is needed.
    expect(setFocusedPane).not.toHaveBeenCalled()
  })

  it('opens the SMB server under the Servers list’s cursor as its share list (the row’s path) in the target pane', async () => {
    // The row is RE-READ: the displayed copy runs a debounce behind a move.
    const left = makePaneRef({
      refreshCursorEntry: vi.fn(() =>
        Promise.resolve({
          isDirectory: true,
          name: 'Testnas',
          path: 'smb://testnas',
          serverMeta: { protocol: 'smb' },
        } as never),
      ),
    })
    const right = makePaneRef()
    const { mirror, navigate } = setup({
      refs: { left, right },
      volumeIdByPane: { left: 'network', right: 'root' },
      pathByPane: { left: 'smb://', right: '/x' },
      focused: 'left',
    })

    mirror.copyPathBetweenPanes({ source: 'left', target: 'right' })

    await vi.waitFor(() => {
      expect(navigate).toHaveBeenCalledWith({
        pane: 'right',
        to: { goTo: { volumeId: 'network', path: 'smb://testnas' } },
        source: 'mirror',
      })
    })
  })

  it('sends the target to the Servers list (a volume select, never a remembered share list) from any other row', async () => {
    const { mirror, navigate } = setup({
      refs: { left: makePaneRef(), right: makePaneRef() },
      volumeIdByPane: { left: 'network', right: 'root' },
      pathByPane: { left: 'smb://', right: '/x' },
      focused: 'left',
    })

    mirror.copyPathBetweenPanes({ source: 'left', target: 'right' })

    await vi.waitFor(() => {
      expect(navigate).toHaveBeenCalledWith({
        pane: 'right',
        to: { selectVolume: { volumeId: 'network', path: 'smb://' } },
        source: 'mirror',
      })
    })
  })

  it('opens the same share list from a share list, mounting the share under the cursor once it lands', async () => {
    net.hosts = [testnas]
    // The displayed copy still names `..` a debounce behind the move; the fresh read names the share.
    const left = makePaneRef({
      getCursorEntry: vi.fn(() => ({ isDirectory: true, name: '..', path: 'smb://' }) as never),
      refreshCursorEntry: vi.fn(() =>
        Promise.resolve({ isDirectory: true, name: 'public', path: 'smb://testnas/public' } as never),
      ),
    })
    const right = makePaneRef()
    const { mirror, navigate } = setup({
      refs: { left, right },
      volumeIdByPane: { left: 'network', right: 'root' },
      pathByPane: { left: 'smb://testnas', right: '/x' },
      focused: 'left',
    })

    mirror.copyPathBetweenPanes({ source: 'left', target: 'right' })

    // ❌ Never the share's `smb://` path: a share is mounted, not navigated to.
    await vi.waitFor(() => {
      expect(right.openShareList).toHaveBeenCalledWith(testnas, { mount: 'public' })
    })
    expect(navigate).not.toHaveBeenCalled()
  })

  it('opens the share list alone with the cursor on `..`', async () => {
    net.hosts = [testnas]
    const left = makePaneRef({
      refreshCursorEntry: vi.fn(() => Promise.resolve({ isDirectory: true, name: '..', path: 'smb://' } as never)),
    })
    const right = makePaneRef()
    const { mirror } = setup({
      refs: { left, right },
      volumeIdByPane: { left: 'network', right: 'root' },
      pathByPane: { left: 'smb://testnas', right: '/x' },
      focused: 'left',
    })

    mirror.copyPathBetweenPanes({ source: 'left', target: 'right' })

    await vi.waitFor(() => {
      expect(right.openShareList).toHaveBeenCalledWith(testnas, { mount: undefined })
    })
  })
})
