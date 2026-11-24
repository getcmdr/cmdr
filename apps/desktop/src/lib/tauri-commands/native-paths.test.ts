/**
 * The native-to-app door (#1311): a Windows drop or pick hands over `C:\…`, which crosses to
 * `C:/…` through the backend before any caller sees it. The backend is mocked as Windows'
 * crossing; `commands/native_paths_test.rs` pins the real answers.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { NativePathCrossing } from '#lib/ipc/bindings.js'

const { cross, open, save, dragDropHandlerRef, cmdrDropHandlerRef } = vi.hoisted(() => ({
  cross: vi.fn<(paths: string[]) => Promise<NativePathCrossing[]>>(),
  open: vi.fn<() => Promise<string | string[] | null>>(),
  save: vi.fn<() => Promise<string | null>>(),
  dragDropHandlerRef: { current: null as ((event: { payload: unknown }) => void) | null },
  cmdrDropHandlerRef: { current: null as ((event: { payload: unknown }) => void) | null },
}))

vi.mock('#lib/ipc/bindings.js', () => ({
  commands: { appPathsFromNative: cross },
  events: {
    // `native-drag-drop`, scoped to the webview: Cmdr's own drop target on Windows.
    nativeDragDrop: () => ({
      listen: (handler: (event: { payload: unknown }) => void) => {
        cmdrDropHandlerRef.current = handler
        return Promise.resolve(() => {})
      },
    }),
  },
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open, save }))
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (handler: (event: { payload: unknown }) => void) => {
      dragDropHandlerRef.current = handler
      return Promise.resolve(() => {})
    },
  }),
}))

import { chooseSaveTarget, onAppPathDragDrop, pickOneAsAppPath } from './native-paths'
import type { AppPathDragDropEvent } from './native-paths'
import { withPlatform } from '#lib/platform/test-platform.js'

/** Windows' crossing, as the backend answers it: a drive path's `\` become `/`; a device path refuses. */
function windowsCrossing(paths: string[]): Promise<NativePathCrossing[]> {
  return Promise.resolve(
    paths.map((path) =>
      path.startsWith('\\\\?\\Volume{') ? { kind: 'refused' } : { kind: 'app', path: path.replaceAll('\\', '/') },
    ),
  )
}

beforeEach(() => {
  cross.mockReset()
  cross.mockImplementation(windowsCrossing)
  open.mockReset()
  save.mockReset()
  dragDropHandlerRef.current = null
  cmdrDropHandlerRef.current = null
  vi.stubGlobal('devicePixelRatio', 1)
  return withPlatform('macos')
})

describe('pickOneAsAppPath', () => {
  it('answers a picked folder as an app path', async () => {
    open.mockResolvedValue('C:\\Users\\test_user_1\\Photos')
    expect(await pickOneAsAppPath({ directory: true })).toBe('C:/Users/test_user_1/Photos')
    expect(open).toHaveBeenCalledWith({ directory: true, multiple: false })
  })

  it('answers null for a cancel, without asking the backend', async () => {
    open.mockResolvedValue(null)
    expect(await pickOneAsAppPath({ directory: true })).toBeNull()
    expect(cross).not.toHaveBeenCalled()
  })

  it('answers null for a pick no app path names', async () => {
    open.mockResolvedValue('\\\\?\\Volume{1}\\x')
    expect(await pickOneAsAppPath({ directory: true })).toBeNull()
  })
})

describe('chooseSaveTarget', () => {
  it('answers the chosen file as an app path', async () => {
    save.mockResolvedValue('C:\\Users\\test_user_1\\notes.selection.txt')
    expect(await chooseSaveTarget({})).toEqual({ kind: 'chosen', path: 'C:/Users/test_user_1/notes.selection.txt' })
  })

  it('tells a cancel from a place no app path names', async () => {
    save.mockResolvedValue(null)
    expect(await chooseSaveTarget({})).toEqual({ kind: 'cancelled' })
    save.mockResolvedValue('\\\\?\\Volume{1}\\x.txt')
    expect(await chooseSaveTarget({})).toEqual({ kind: 'refused' })
  })
})

describe('onAppPathDragDrop', () => {
  async function subscribe(): Promise<{ events: AppPathDragDropEvent[]; fire: (payload: unknown) => void }> {
    const events: AppPathDragDropEvent[] = []
    await onAppPathDragDrop((event) => events.push(event))
    const handler = dragDropHandlerRef.current
    if (!handler) throw new Error('no drag-drop subscription')
    return {
      events,
      fire: (payload) => {
        handler({ payload })
      },
    }
  }

  it('delivers a drop from Explorer with app paths', async () => {
    const { events, fire } = await subscribe()
    fire({ type: 'drop', paths: ['C:\\Users\\test_user_1\\a.txt'], position: { x: 1, y: 2 } })
    await vi.waitFor(() => {
      expect(events).toHaveLength(1)
    })
    expect(events[0]).toEqual({
      type: 'drop',
      paths: ['C:/Users/test_user_1/a.txt'],
      refused: 0,
      position: { x: 1, y: 2 },
    })
  })

  it('crosses each path and counts the ones no app path names', async () => {
    const { events, fire } = await subscribe()
    fire({
      type: 'enter',
      paths: ['C:\\Users\\test_user_1\\a.txt', '\\\\?\\Volume{1}\\x', 'D:\\b'],
      position: { x: 0, y: 0 },
    })
    await vi.waitFor(() => {
      expect(events).toHaveLength(1)
    })
    expect(events[0]).toMatchObject({ paths: ['C:/Users/test_user_1/a.txt', 'D:/b'], refused: 1 })
  })

  it('asks the backend nothing for a drop with no paths (a browser image)', async () => {
    const { events, fire } = await subscribe()
    fire({ type: 'drop', paths: [], position: { x: 0, y: 0 } })
    await vi.waitFor(() => {
      expect(events).toHaveLength(1)
    })
    expect(events[0]).toMatchObject({ paths: [], refused: 0 })
    expect(cross).not.toHaveBeenCalled()
  })

  it('keeps the OS order: an over waits for the enter before it to cross', async () => {
    let release: () => void = () => {}
    cross.mockImplementationOnce(
      (paths) =>
        new Promise((resolve) => {
          release = () => {
            void windowsCrossing(paths).then(resolve)
          }
        }),
    )
    const { events, fire } = await subscribe()
    fire({ type: 'enter', paths: ['C:\\a'], position: { x: 0, y: 0 } })
    fire({ type: 'over', position: { x: 5, y: 5 } })
    fire({ type: 'leave' })
    await Promise.resolve()
    expect(events).toEqual([])
    release()
    await vi.waitFor(() => {
      expect(events).toHaveLength(3)
    })
    expect(events.map((event) => event.type)).toEqual(['enter', 'over', 'leave'])
    expect(events[0]).toMatchObject({ paths: ['C:/a'], refused: 0 })
  })

  it('keeps delivering after a handler throws, so one bad event never ends every later drag', async () => {
    const events: AppPathDragDropEvent[] = []
    let first = true
    await onAppPathDragDrop((event) => {
      if (first) {
        first = false
        throw new Error('a handler bug')
      }
      events.push(event)
    })
    const handler = dragDropHandlerRef.current
    if (!handler) throw new Error('no drag-drop subscription')
    handler({ payload: { type: 'leave' } })
    handler({ payload: { type: 'drop', paths: ['C:\\a'], position: { x: 0, y: 0 } } })
    await vi.waitFor(() => {
      expect(events).toHaveLength(1)
    })
    expect(events[0]).toMatchObject({ type: 'drop', paths: ['C:/a'] })
  })

  it('counts every path refused when the bridge is broken, so no native text slips by', async () => {
    cross.mockRejectedValueOnce(new Error('bridge down'))
    const { events, fire } = await subscribe()
    fire({ type: 'drop', paths: ['C:\\a', 'C:\\b'], position: { x: 0, y: 0 } })
    await vi.waitFor(() => {
      expect(events).toHaveLength(1)
    })
    expect(events[0]).toMatchObject({ paths: [], refused: 2 })
  })
})

describe('onAppPathDragDrop on Windows (#1311)', () => {
  beforeEach(() => withPlatform('windows'))

  it('reads positions in physical pixels, so at 150 % a drop hits the row under the cursor', async () => {
    vi.stubGlobal('devicePixelRatio', 1.5)
    const events: AppPathDragDropEvent[] = []
    await onAppPathDragDrop((event) => events.push(event))
    dragDropHandlerRef.current?.({ payload: { type: 'over', position: { x: 300, y: 150 } } })
    await vi.waitFor(() => {
      expect(events).toHaveLength(1)
    })
    expect(events[0]).toEqual({ type: 'over', position: { x: 200, y: 100 } })
  })

  it("delivers Cmdr's own drop target's events, whose paths are app paths already", async () => {
    vi.stubGlobal('devicePixelRatio', 2)
    const events: AppPathDragDropEvent[] = []
    await onAppPathDragDrop((event) => events.push(event))
    const fire = cmdrDropHandlerRef.current
    if (!fire) throw new Error('no native-drag-drop subscription on Windows')
    fire({ payload: { type: 'enter', paths: ['C:/Users/test_user_1/a.txt'], refused: 1, position: { x: 40, y: 20 } } })
    fire({ payload: { type: 'over', position: { x: 42, y: 20 } } })
    fire({ payload: { type: 'leave' } })
    await vi.waitFor(() => {
      expect(events).toHaveLength(3)
    })
    expect(events).toEqual([
      { type: 'enter', paths: ['C:/Users/test_user_1/a.txt'], refused: 1, position: { x: 20, y: 10 } },
      { type: 'over', position: { x: 21, y: 10 } },
      { type: 'leave' },
    ])
    expect(cross).not.toHaveBeenCalled()
  })

  it("listens to Cmdr's drop target only on Windows", async () => {
    withPlatform('macos')
    await onAppPathDragDrop(() => {})
    expect(cmdrDropHandlerRef.current).toBeNull()
  })
})
