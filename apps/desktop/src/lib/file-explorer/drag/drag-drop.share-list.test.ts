/**
 * A drag out of a server's share list takes nothing out: its rows are shares, not files,
 * so the one source guard (`pane/transfer-entry.ts::checkTransferSourceGuard`) refuses
 * at the threshold, says why, and leaves the press a click.
 */
import { describe, it, expect, beforeEach, vi } from 'vitest'

const { dragged, toasts } = vi.hoisted(() => ({ dragged: [] as string[], toasts: [] as string[] }))

vi.mock('@tauri-apps/api/path', () => ({
  tempDir: () => Promise.resolve('/tmp'),
  join: (...parts: string[]) => Promise.resolve(parts.join('/')),
}))
vi.mock('@tauri-apps/plugin-fs', () => ({ writeFile: () => Promise.resolve() }))
vi.mock('#lib/icon-cache.js', () => ({ getCachedIcon: () => 'data:image/png;base64,AA==' }))
vi.mock('#lib/tauri-commands/index.js', () => ({
  DEFAULT_VOLUME_ID: 'root',
  startDragPaths: (paths: string[]) => {
    dragged.push(...paths)
    return Promise.resolve()
  },
  prepareSelfDragOverlay: () => Promise.resolve(),
  clearSelfDragOverlay: () => Promise.resolve(),
  setSelfDragResolvedOperation: () => Promise.resolve(),
}))
vi.mock('#lib/ui/toast/index.js', () => ({
  addToast: (message: string) => {
    toasts.push(message)
    return 'toast'
  },
}))
vi.mock('#lib/settings/settings-store.js', () => ({ getSetting: vi.fn(() => 5) }))
vi.mock('../rename/rename-activation', () => ({ cancelClickToRename: vi.fn() }))
vi.mock('./drag-image-renderer', () => ({ renderDragImage: () => Promise.reject(new Error('no canvas here')) }))

import { startSelectionDragTracking, cancelDragTracking } from './drag-drop'

/** Long enough for a started drag's async chain to reach `startDragPaths`. */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 50))
}

describe("a drag out of a server's share list", () => {
  beforeEach(() => {
    dragged.length = 0
    toasts.length = 0
    cancelDragTracking()
  })

  it('refuses at the threshold with the share-list words, and the press stays a click', async () => {
    const onDragCancel = vi.fn()
    const onDragStart = vi.fn()
    startSelectionDragTracking(
      new MouseEvent('mousedown', { clientX: 0, clientY: 0 }),
      {
        type: 'single',
        path: 'smb://testnas/public',
        iconId: 'dir',
        index: 1,
        sourceVolumeId: 'network',
        sourcePath: 'smb://testnas',
      },
      { onDragCancel, onDragStart },
    )

    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 40, clientY: 0 }))
    await settle()

    expect(dragged).toEqual([])
    expect(onDragStart).not.toHaveBeenCalled()
    expect(onDragCancel).toHaveBeenCalledOnce()
    expect(toasts).toEqual([
      'A share is a place on the server, so there’s nothing here to copy. Open the share to copy files from it.',
    ])
  })

  it('refuses a Servers row in the Servers list’s words, though the row’s own path is a share list', async () => {
    const onDragCancel = vi.fn()
    startSelectionDragTracking(
      new MouseEvent('mousedown', { clientX: 0, clientY: 0 }),
      {
        type: 'single',
        path: 'smb://testnas',
        iconId: 'dir',
        index: 0,
        sourceVolumeId: 'network',
        sourcePath: 'smb://',
      },
      { onDragCancel },
    )

    document.dispatchEvent(new MouseEvent('mousemove', { clientX: 40, clientY: 0 }))
    await settle()

    expect(dragged).toEqual([])
    expect(toasts).toEqual([
      'A server is a place on the network, so there’s nothing here to copy. Open the server, then one of its shares, to copy files from it.',
    ])
  })
})
