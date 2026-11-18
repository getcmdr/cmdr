/**
 * Startup wiring for a restored tab whose volume the startup sweep hasn't registered yet.
 *
 * ❗ On Windows a stick that was on `E:` may be on `F:` now, with another disk on `E:`. Until
 * the sweep registers the stick, nothing can tell, so the volume lookup would file the tab
 * under whatever sits at `E:` now, and the tab would be saved with that disk's id for good.
 * A pending tab keeps its stored id and path and shows as unreachable instead.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { getActiveTab } from '../tabs/tab-state-manager.svelte'
import type { LoadedPaneTabs } from '#lib/app-status-store.js'

const appStatus = vi.hoisted(() => ({
  loadAppStatus: vi.fn(),
  loadPaneTabs: vi.fn(),
  hasPersistedPaneState: vi.fn(),
  saveAppStatusNow: vi.fn(),
  savePaneTabs: vi.fn(),
  resolvePersistedPath: vi.fn(),
}))

const commands = vi.hoisted(() => ({
  pathExists: vi.fn(),
  getDefaultVolumeId: vi.fn(),
  resolvePathVolume: vi.fn(),
  liveVolumePath: vi.fn(),
  getE2eStartPath: vi.fn(),
  checkFullDiskAccessQuiet: vi.fn(),
  listSavedServers: vi.fn(),
}))

vi.mock('#lib/app-status-store.js', () => appStatus)
vi.mock('#lib/tauri-commands/index.js', () => commands)
vi.mock('#lib/app-mode.js', () => ({ isE2eRun: () => false }))
vi.mock('#lib/ask-cmdr/ask-cmdr-trigger.svelte.js', () => ({ hydrateRail: vi.fn() }))

function paneTabs(side: 'left' | 'right', path: string, volumeId: string, pending: boolean): LoadedPaneTabs {
  const id = `${side}-tab`
  return {
    tabs: [{ id, path, volumeId, sortBy: 'name', sortOrder: 'ascending', viewMode: 'brief', pinned: false }],
    activeTabId: id,
    registryPendingTabIds: pending ? [id] : [],
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  appStatus.loadPaneTabs.mockImplementation((side: 'left' | 'right') =>
    Promise.resolve(
      side === 'left' ? paneTabs(side, 'E:/photos', 'vol-stick-a', true) : paneTabs(side, 'C:/Users', 'root', false),
    ),
  )
  appStatus.loadAppStatus.mockResolvedValue({
    focusedPane: 'left',
    leftPaneWidthPercent: 50,
    askCmdrRailOpen: false,
    askCmdrRailWidth: 340,
    firstRunLayoutApplied: true,
  })
  appStatus.hasPersistedPaneState.mockResolvedValue(true)
  commands.getDefaultVolumeId.mockResolvedValue('root')
  commands.getE2eStartPath.mockResolvedValue(null)
  commands.checkFullDiskAccessQuiet.mockResolvedValue(true)
  // The disk that took `E:` answers for the old path.
  commands.resolvePathVolume.mockResolvedValue({ volume: { id: 'vol-disk-b' }, timedOut: false })
})

describe('loadPersistedState: a tab whose volume isn’t registered yet', () => {
  it('keeps its stored id and path, unreachable, and never asks which volume holds the path', async () => {
    const { loadPersistedState } = await import('./initialization')
    const state = await loadPersistedState()
    const tab = getActiveTab(state.leftTabMgr)

    expect(tab.volumeId).toBe('vol-stick-a')
    expect(tab.path).toBe('E:/photos')
    expect(tab.unreachable).toMatchObject({ originalPath: 'E:/photos' })
    expect(commands.resolvePathVolume).not.toHaveBeenCalledWith('E:/photos')
    expect(getActiveTab(state.rightTabMgr).volumeId).toBe('vol-disk-b')
  })
})
