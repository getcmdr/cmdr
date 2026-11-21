/**
 * The composer drop resolver's branch logic: an in-app LOCAL self-drag uses the recorded
 * identity paths; a VIRTUAL self-drag is dropped (its round-tripped paths mis-resolve); a
 * Finder drop uses the genuine payload paths. `installComposerDrop` is a no-op outside a
 * Tauri webview.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { AttachmentRef } from '#lib/tauri-commands/index.js'
import type { SelfDragIdentity } from '#lib/file-explorer/drag/drag-drop.js'
import { tString } from '#lib/intl/messages.svelte.js'

const resolveMock = vi.fn<(paths: string[]) => Promise<AttachmentRef[]>>()
const state = { isSelf: false, identity: null as SelfDragIdentity | null }
const { addToast, dragDropHandlerRef } = vi.hoisted(() => ({
  addToast: vi.fn(),
  dragDropHandlerRef: { current: null as ((event: { payload: unknown }) => void) | null },
}))

vi.mock('#lib/tauri-commands/index.js', async () => ({
  resolveAskCmdrAttachments: (paths: string[]) => resolveMock(paths),
  // The real door, over a mocked webview and a mocked backend crossing below.
  onAppPathDragDrop: (
    await vi.importActual<typeof import('#lib/tauri-commands/native-paths.js')>('#lib/tauri-commands/native-paths.js')
  ).onAppPathDragDrop,
}))
// Windows' crossing, as the backend answers it (`commands/native_paths_test.rs` pins the real one).
vi.mock('#lib/ipc/bindings.js', () => ({
  commands: {
    appPathsFromNative: (paths: string[]) =>
      Promise.resolve(paths.map((path) => ({ kind: 'app', path: path.replaceAll('\\', '/') }))),
  },
}))
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (handler: (event: { payload: unknown }) => void) => {
      dragDropHandlerRef.current = handler
      return Promise.resolve(() => {})
    },
  }),
}))
vi.mock('#lib/ui/toast/index.js', () => ({ addToast }))
vi.mock('#lib/file-explorer/drag/drag-drop.js', () => ({
  getIsDraggingFromSelf: () => state.isSelf,
  getSelfDragIdentity: () => state.identity,
}))
// Corrects only a DevTools-docking offset; identity passthrough in tests.
vi.mock('#lib/file-explorer/drag/drag-position.js', () => ({
  toViewportPosition: (p: { x: number; y: number }) => p,
}))

import {
  handleDragDropEvent,
  installComposerDrop,
  isWithinRect,
  resolveDroppedPaths,
  type DropHandlers,
} from './ask-cmdr-drop'

function identity(sourceVolumeId: string, sourcePaths: string[]): SelfDragIdentity {
  return { sourceVolumeId, sourcePaths, startedAt: 0 }
}

/** A composer rect at (100,100)–(300,200). */
function rect(): DOMRect {
  return { left: 100, top: 100, right: 300, bottom: 200, x: 100, y: 100, width: 200, height: 100, toJSON: () => ({}) }
}

function handlers(overrides: Partial<DropHandlers> = {}): DropHandlers & {
  active: boolean[]
  attached: AttachmentRef[][]
} {
  const active: boolean[] = []
  const attached: AttachmentRef[][] = []
  return {
    getRect: () => rect(),
    onDragActive: (a) => active.push(a),
    onAttachments: (refs) => attached.push(refs),
    active,
    attached,
    ...overrides,
  }
}

beforeEach(() => {
  resolveMock.mockReset()
  resolveMock.mockImplementation((paths) => Promise.resolve(paths.map((path) => ({ path, kind: 'file' as const }))))
  state.isSelf = false
  state.identity = null
})

describe('isWithinRect', () => {
  it('is true for a point inside the rect and on its edges', () => {
    expect(isWithinRect({ x: 150, y: 150 }, rect())).toBe(true)
    expect(isWithinRect({ x: 100, y: 100 }, rect())).toBe(true) // top-left corner
    expect(isWithinRect({ x: 300, y: 200 }, rect())).toBe(true) // bottom-right corner
  })

  it('is false for a point outside the rect', () => {
    expect(isWithinRect({ x: 50, y: 150 }, rect())).toBe(false) // left of
    expect(isWithinRect({ x: 150, y: 250 }, rect())).toBe(false) // below
  })

  it('is false when the composer is unmounted (null rect)', () => {
    expect(isWithinRect({ x: 150, y: 150 }, null)).toBe(false)
  })
})

describe('handleDragDropEvent', () => {
  it('enter/over inside the composer activate the overlay', async () => {
    const h = handlers()
    await handleDragDropEvent({ type: 'enter', paths: ['/a'], position: { x: 150, y: 150 } }, h)
    await handleDragDropEvent({ type: 'over', position: { x: 160, y: 160 } }, h)
    expect(h.active).toEqual([true, true])
  })

  it('enter/over outside the composer deactivate the overlay', async () => {
    const h = handlers()
    await handleDragDropEvent({ type: 'over', position: { x: 10, y: 10 } }, h)
    expect(h.active).toEqual([false])
  })

  it('leave always clears the overlay', async () => {
    const h = handlers()
    await handleDragDropEvent({ type: 'leave' }, h)
    expect(h.active).toEqual([false])
    expect(h.attached).toEqual([])
  })

  it('a drop inside resolves paths and clears the overlay', async () => {
    state.isSelf = false
    const h = handlers()
    await handleDragDropEvent({ type: 'drop', paths: ['/Users/d/f.txt'], position: { x: 150, y: 150 } }, h)
    expect(h.active).toEqual([false])
    expect(h.attached).toEqual([[{ path: '/Users/d/f.txt', kind: 'file' }]])
  })

  it('a drop OUTSIDE the composer clears the overlay but resolves nothing', async () => {
    const h = handlers()
    await handleDragDropEvent({ type: 'drop', paths: ['/Users/d/f.txt'], position: { x: 10, y: 10 } }, h)
    expect(h.active).toEqual([false])
    expect(h.attached).toEqual([])
    expect(resolveMock).not.toHaveBeenCalled()
  })

  it('a drop inside from a local self-drag uses the identity paths', async () => {
    state.isSelf = true
    state.identity = identity('root', ['/Users/d/a', '/Users/d/b'])
    const h = handlers()
    await handleDragDropEvent({ type: 'drop', paths: ['/round-tripped'], position: { x: 150, y: 150 } }, h)
    expect(resolveMock).toHaveBeenCalledWith(['/Users/d/a', '/Users/d/b'])
    expect(h.attached[0].map((r) => r.path)).toEqual(['/Users/d/a', '/Users/d/b'])
  })
})

describe('resolveDroppedPaths', () => {
  it('a local self-drag resolves the recorded identity paths (not the payload paths)', async () => {
    state.isSelf = true
    state.identity = identity('root', ['/Users/d/a', '/Users/d/b'])
    const refs = await resolveDroppedPaths(['/wrong/payload/path'])
    expect(resolveMock).toHaveBeenCalledWith(['/Users/d/a', '/Users/d/b'])
    expect(refs.map((r) => r.path)).toEqual(['/Users/d/a', '/Users/d/b'])
  })

  it('a virtual-volume self-drag is dropped (paths would mis-resolve)', async () => {
    state.isSelf = true
    state.identity = identity('smb-share', ['/photos/sunset.jpg'])
    const refs = await resolveDroppedPaths(['/photos/sunset.jpg'])
    expect(resolveMock).not.toHaveBeenCalled()
    expect(refs).toEqual([])
  })

  it('a Finder (external) drop uses the genuine payload paths', async () => {
    state.isSelf = false
    const refs = await resolveDroppedPaths(['/Users/d/from-finder.pdf'])
    expect(resolveMock).toHaveBeenCalledWith(['/Users/d/from-finder.pdf'])
    expect(refs.map((r) => r.path)).toEqual(['/Users/d/from-finder.pdf'])
  })

  it('an external drop with a path that had no app path attaches nothing, and says so', async () => {
    const refs = await resolveDroppedPaths(['C:/Users/d/a.pdf'], 1)
    expect(resolveMock).not.toHaveBeenCalled()
    expect(refs).toEqual([])
    expect(addToast).toHaveBeenCalledWith(tString('fileExplorer.drop.uncrossedPaths'), { level: 'warn' })
  })

  it('an empty external drop resolves to nothing', async () => {
    const refs = await resolveDroppedPaths([])
    expect(resolveMock).not.toHaveBeenCalled()
    expect(refs).toEqual([])
  })
})

describe('installComposerDrop', () => {
  it('is a no-op (returns a callable unlisten) outside a Tauri webview', async () => {
    const unlisten = await installComposerDrop(
      () => null,
      () => {},
      () => {},
    )
    expect(typeof unlisten).toBe('function')
    expect(() => {
      unlisten()
    }).not.toThrow()
  })

  it('attaches an Explorer drop by its app paths, never the native `C:\\` text (#1311)', async () => {
    const h = handlers()
    await installComposerDrop(h.getRect, h.onDragActive, h.onAttachments)
    dragDropHandlerRef.current?.({
      payload: { type: 'drop', paths: ['C:\\Users\\test_user_1\\report.pdf'], position: { x: 150, y: 150 } },
    })
    await vi.waitFor(() => {
      expect(h.attached).toHaveLength(1)
    })
    expect(resolveMock).toHaveBeenCalledWith(['C:/Users/test_user_1/report.pdf'])
  })
})
