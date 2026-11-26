import type { NavigationHistory } from '../navigation/navigation-history'
import type { SortColumn, SortOrder } from '../types'
import type { ViewMode } from '#lib/app-status-store.js'
import type { Location } from '#lib/tauri-commands/index.js'

export type TabId = string // crypto.randomUUID()

/** Tracks a tab whose volume couldn't be resolved at startup (timeout or unreachable path). */
export interface UnreachableState {
  /** The original path the tab was trying to restore */
  originalPath: string
  /** Whether a retry is currently in progress */
  retrying: boolean
}

/** Full runtime state for one tab. Composes `Location` (the tab's volumeId + path). */
export interface TabState extends Location {
  id: TabId
  history: NavigationHistory
  sortBy: SortColumn
  sortOrder: SortOrder
  viewMode: ViewMode
  pinned: boolean
  /** Saved on switch-away, restored on switch-to */
  cursorFilename: string | null
  /** Set when the tab's volume resolution timed out at startup */
  unreachable: UnreachableState | null
  /**
   * The share-list path (`smb://<server>`) the tab's Servers hub puts its cursor on, once:
   * set by the commit that takes the tab from that share list to the hub, or by a restore
   * that brought a share-list tab back as the hub. Consumed by the hub's first render of
   * this tab, dropped by any other path change. Never persisted. `pane/hub-landing.svelte.ts`.
   */
  landOnServer?: string
}

/**
 * Stored in app-status.json per tab. Composes `Location`, which keeps the
 * `volumeId` + `path` field names byte-identical so persisted tabs round-trip.
 */
export interface PersistedTab extends Location {
  id: TabId
  sortBy: SortColumn
  sortOrder: SortOrder
  viewMode: ViewMode
  pinned: boolean
}

/** Stored in app-status.json per pane side */
export interface PersistedPaneTabs {
  tabs: PersistedTab[]
  activeTabId: TabId
}
