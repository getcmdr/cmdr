/**
 * A selection drag whose rows moved under it: a watcher insert lands above the
 * selection between the pane applying its rows and the drag threshold, so the
 * old row numbers name the neighbor (`pane/moved-listing.test-fixtures.ts`).
 * Dragging that one to another pane or the Trash moves a file nobody picked.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'

const { dragged, addToastSpy, snapshotSpy } = vi.hoisted(() => ({
  dragged: [] as string[],
  addToastSpy: vi.fn(),
  snapshotSpy: vi.fn(),
}))

vi.mock('@tauri-apps/api/path', () => ({
  tempDir: () => Promise.resolve('/tmp'),
  join: (...parts: string[]) => Promise.resolve(parts.join('/')),
}))
vi.mock('@tauri-apps/plugin-fs', () => ({ writeFile: () => Promise.resolve() }))
vi.mock('@tauri-apps/api/event', () => ({ emit: vi.fn() }))
vi.mock('#lib/icon-cache.js', () => ({
  getCachedIcon: () => 'data:image/png;base64,AA==',
  getCachedCustomFolderIcon: () => undefined,
}))
vi.mock('#lib/tauri-commands/index.js', async () => {
  const { asListingLookupError } = await vi.importActual<typeof import('#lib/tauri-commands/listing-gone.js')>(
    '#lib/tauri-commands/listing-gone.js',
  )
  return {
    asListingLookupError,
    getSelectionSnapshot: snapshotSpy,
    startDragPaths: (paths: string[]) => {
      dragged.push(...paths)
      return Promise.resolve()
    },
    prepareSelfDragOverlay: () => Promise.resolve(),
    clearSelfDragOverlay: () => Promise.resolve(),
    setSelfDragResolvedOperation: () => Promise.resolve(),
  }
})
vi.mock('#lib/ui/toast/index.js', () => ({ addToast: addToastSpy }))
vi.mock('#lib/settings/settings-store.js', () => ({ getSetting: vi.fn(() => 5) }))
vi.mock('../rename/rename-activation', () => ({ cancelClickToRename: vi.fn() }))
vi.mock('./drag-image-renderer', () => ({ renderDragImage: () => Promise.reject(new Error('no canvas here')) }))

import { startSelectionDragTracking, getSelfDragIdentity, cancelDragTracking } from './drag-drop'
import {
  SELECTED_PATH,
  catchingUpAfterRefusal,
  createMovedPane,
  selectionSnapshot,
} from '../pane/moved-listing.test-fixtures'

/** Presses on a selected row and moves past the drag threshold. */
function dragSelection(pane: ReturnType<typeof createMovedPane>) {
  startSelectionDragTracking(new MouseEvent('mousedown', { clientX: 0, clientY: 0 }), {
    type: 'selection',
    pane,
    includeHidden: false,
    sourceVolumeId: 'root',
    sourcePath: '/Users/test_user_1',
    iconId: 'ext:txt',
  })
  document.dispatchEvent(new MouseEvent('mousemove', { clientX: 50, clientY: 0 }))
}

describe('a selection drag when the rows moved under the press', () => {
  beforeEach(() => {
    dragged.length = 0
    addToastSpy.mockClear()
    cancelDragTracking()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('waits for the pane to apply the diff and drags the file it picked', async () => {
    const pane = createMovedPane({ onSelection: true })
    snapshotSpy.mockImplementation(catchingUpAfterRefusal(pane, selectionSnapshot))

    dragSelection(pane)

    await vi.waitFor(() => {
      expect(dragged).toEqual([SELECTED_PATH])
    })
    expect(getSelfDragIdentity()?.sourcePaths).toEqual([SELECTED_PATH])
    expect(addToastSpy).not.toHaveBeenCalled()
  })

  it('refuses with the toast once the rows never settle, dragging nothing', async () => {
    vi.useFakeTimers()
    snapshotSpy.mockImplementation(selectionSnapshot)

    dragSelection(createMovedPane({ onSelection: true }))
    await vi.advanceTimersByTimeAsync(3_500)

    expect(dragged).toEqual([])
    expect(getSelfDragIdentity()).toBeNull()
    expect(addToastSpy).toHaveBeenCalledWith(
      'The list here is still updating, so nothing started. Try again in a moment.',
      { level: 'info' },
    )
  })
})
