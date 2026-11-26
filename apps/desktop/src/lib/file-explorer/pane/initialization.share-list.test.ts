/**
 * Startup wiring for a tab that was on a server's share list: the store hands it back
 * as the Servers list plus the server it lands on (`app-status-store.ts`), and this
 * carries that landing past `toPersistedTab`'s strip onto the live tab, where the tab's
 * first listing takes it (`restored-landing.svelte.ts`).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { getActiveTab, getAllTabs } from '../tabs/tab-state-manager.svelte'
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

const tab = (id: string, path: string, volumeId: string) =>
  ({ id, path, volumeId, sortBy: 'name', sortOrder: 'ascending', viewMode: 'brief', pinned: false }) as const

beforeEach(() => {
  vi.clearAllMocks()
  appStatus.loadPaneTabs.mockImplementation((side: 'left' | 'right') =>
    Promise.resolve<LoadedPaneTabs>(
      side === 'left'
        ? {
            // The active tab and a background one were both share lists.
            tabs: [tab('nas', 'smb://', 'network'), tab('attic', 'smb://', 'network'), tab('home', '~', 'root')],
            activeTabId: 'nas',
            registryPendingTabIds: [],
            landOnServerByTabId: { nas: 'smb://testnas', attic: 'smb://attic' },
          }
        : { tabs: [tab('r', '~', 'root')], activeTabId: 'r', registryPendingTabIds: [] },
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
  commands.resolvePathVolume.mockResolvedValue({ volume: { id: 'root' }, timedOut: false })
})

describe('loadPersistedState: a tab that was on a share list', () => {
  it('opens on the Servers list, active and background tabs alike, each carrying the server it lands on', async () => {
    const { loadPersistedState } = await import('./initialization')
    const state = await loadPersistedState()

    expect(getActiveTab(state.leftTabMgr)).toMatchObject({
      volumeId: 'network',
      path: 'smb://',
      landOn: { path: 'smb://', selectPath: 'smb://testnas' },
    })
    const byId = Object.fromEntries(getAllTabs(state.leftTabMgr).map((t) => [t.id, t.landOn?.selectPath]))
    expect(byId).toEqual({ nas: 'smb://testnas', attic: 'smb://attic', home: undefined })
    // Nothing asked which volume holds a server: `network` is trusted as stored.
    expect(commands.resolvePathVolume).not.toHaveBeenCalledWith('smb://')
  })
})
