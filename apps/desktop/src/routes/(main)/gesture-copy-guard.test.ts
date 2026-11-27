/**
 * The gesture copy guard: a file-scope ⌘C that lands right after a double-click or a window
 * drag is a selection utility's synthetic keystroke, not the user (ERR-YTH9K).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const { debug } = vi.hoisted(() => ({ debug: vi.fn() }))
vi.mock('#lib/logging/logger.js', () => ({
  getAppLogger: () => ({ info: vi.fn(), debug, warn: vi.fn(), error: vi.fn() }),
}))

import {
  GESTURE_COPY_GUARD_MS,
  noteMouseGesture,
  shouldIgnoreFileCopy,
  startGestureCopyGuard,
  _resetGestureCopyGuardForTests,
} from './gesture-copy-guard'

beforeEach(() => {
  _resetGestureCopyGuardForTests()
  debug.mockClear()
})

describe('shouldIgnoreFileCopy', () => {
  it('lets a copy through when no gesture happened', () => {
    expect(shouldIgnoreFileCopy('menu', 1_000)).toBe(false)
  })

  it('ignores a menu-road copy right after a double-click, and logs it at debug', () => {
    noteMouseGesture('doubleClick', 1_000)
    expect(shouldIgnoreFileCopy('menu', 1_080)).toBe(true)
    expect(debug).toHaveBeenCalledOnce()
  })

  it('ignores a keyboard-road copy right after a window drag', () => {
    noteMouseGesture('windowDrag', 1_000)
    expect(shouldIgnoreFileCopy('keyboard', 1_020)).toBe(true)
  })

  it('lets a copy through once the guard window has passed', () => {
    noteMouseGesture('doubleClick', 1_000)
    expect(shouldIgnoreFileCopy('menu', 1_000 + GESTURE_COPY_GUARD_MS)).toBe(false)
    expect(debug).not.toHaveBeenCalled()
  })

  it('measures from the latest gesture (a long drag keeps the window open)', () => {
    noteMouseGesture('windowDrag', 1_000)
    noteMouseGesture('windowDrag', 1_400)
    expect(shouldIgnoreFileCopy('menu', 1_800)).toBe(true)
  })

  it.each(['palette', 'explorer', 'mcp', 'contextMenu', 'barClick', 'mouse'] as const)(
    'never ignores a copy from the %s road, which no synthetic keystroke can take',
    (source) => {
      noteMouseGesture('doubleClick', 1_000)
      expect(shouldIgnoreFileCopy(source, 1_010)).toBe(false)
    },
  )
})

describe('startGestureCopyGuard', () => {
  function setup() {
    const target = new EventTarget()
    let moved: (() => void) | null = null
    const unlistenMoved = vi.fn()
    const onWindowMoved = vi.fn((cb: () => void) => {
      moved = cb
      return Promise.resolve(unlistenMoved)
    })
    return { target, onWindowMoved, unlistenMoved, fireMoved: () => moved?.() }
  }

  function mouseup(init: MouseEventInit): MouseEvent {
    return new MouseEvent('mouseup', init)
  }

  it('notes a left-button double-click', async () => {
    const { target, onWindowMoved } = setup()
    await startGestureCopyGuard({ target, onWindowMoved })
    target.dispatchEvent(mouseup({ button: 0, detail: 2 }))
    expect(shouldIgnoreFileCopy('menu')).toBe(true)
  })

  it('notes a triple-click too (selection utilities fire on it as well)', async () => {
    const { target, onWindowMoved } = setup()
    await startGestureCopyGuard({ target, onWindowMoved })
    target.dispatchEvent(mouseup({ button: 0, detail: 3 }))
    expect(shouldIgnoreFileCopy('menu')).toBe(true)
  })

  it('ignores a single click and a right-button double-click', async () => {
    const { target, onWindowMoved } = setup()
    await startGestureCopyGuard({ target, onWindowMoved })
    target.dispatchEvent(mouseup({ button: 0, detail: 1 }))
    target.dispatchEvent(mouseup({ button: 2, detail: 2 }))
    expect(shouldIgnoreFileCopy('menu')).toBe(false)
  })

  it('notes a window move as a window drag', async () => {
    const { target, onWindowMoved, fireMoved } = setup()
    await startGestureCopyGuard({ target, onWindowMoved })
    fireMoved()
    expect(shouldIgnoreFileCopy('menu')).toBe(true)
  })

  it('stops listening on teardown', async () => {
    const { target, onWindowMoved, unlistenMoved } = setup()
    const stop = await startGestureCopyGuard({ target, onWindowMoved })
    stop()
    target.dispatchEvent(mouseup({ button: 0, detail: 2 }))
    expect(shouldIgnoreFileCopy('menu')).toBe(false)
    expect(unlistenMoved).toHaveBeenCalledOnce()
  })

  it('still guards double-clicks when the window-move subscription is unavailable (outside Tauri)', async () => {
    const target = new EventTarget()
    const stop = await startGestureCopyGuard({ target, onWindowMoved: () => Promise.reject(new Error('no Tauri')) })
    target.dispatchEvent(mouseup({ button: 0, detail: 2 }))
    expect(shouldIgnoreFileCopy('menu')).toBe(true)
    stop()
  })
})
