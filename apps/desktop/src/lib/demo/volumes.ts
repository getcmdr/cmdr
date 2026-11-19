/**
 * The demo's volume list: one main disk plus the usual favorites, all on the in-memory volume. The
 * disk wears the name the desktop gives its main volume on the visitor's OS.
 */
import type { LocationCategory, LocationInfo } from '#lib/ipc/bindings.js'
import type { Platform } from '#lib/platform/index.js'
import type { AnswerTable } from './answer-types.js'
import { DEMO_HOME, DEMO_VOLUME_ID } from './demo-tree.js'
import { visitorPlatform } from './platform.js'

/**
 * The main volume as the desktop names it on each OS: the Mac's startup disk; the system drive as
 * `WindowsDrive::display_name` reads an unlabeled one (`crates/cmdr-fs/src/volume/windows_volume_ids.rs`,
 * `C:`); Linux's root row (`get_main_volume` in `src-tauri/src/volumes/platform/linux/mod.rs`,
 * `Root`). Its file system too, which Settings and the volume's tooltip name. The demo's paths
 * stay POSIX for every visitor (`docs/specs/windows-port-plan.md`, task 63 step 11).
 */
const MAIN_VOLUME: Readonly<Record<Platform, { name: string; fsType: string }>> = {
  macos: { name: 'Macintosh HD', fsType: 'apfs' },
  windows: { name: 'C:', fsType: 'ntfs' },
  linux: { name: 'Root', fsType: 'ext4' },
}

/** The main volume's name for a visitor on `platform` (the visitor's own by default). */
export function mainVolumeName(platform: Platform = visitorPlatform(navigator.userAgent)): string {
  return MAIN_VOLUME[platform].name
}

function location(
  platform: Platform,
  id: string,
  name: string,
  path: string,
  category: LocationCategory,
  favoriteShortcut: string | null = null,
): LocationInfo {
  return {
    id,
    name,
    path,
    category,
    icon: null,
    isEjectable: false,
    fsType: MAIN_VOLUME[platform].fsType,
    // The in-memory volume has no Trash (`Volume::has_trash`), so F8 deletes for good after the
    // app's own confirm, as on a network share.
    supportsTrash: false,
    // Writes land, in memory, through the real write engine (`crates/cmdr-listing-wasm`).
    mountIsReadOnly: false,
    isDiskImage: false,
    sharesDiskWithOthers: false,
    isCloudMount: false,
    // The in-memory volume compares names exactly.
    caseSensitive: true,
    connectionState: null,
    pinned: null,
    landingPath: null,
    deviceReadiness: null,
    usbSpeed: null,
    capabilities: null,
    favoriteShortcut,
    favoriteTarget:
      category === 'favorite'
        ? {
            volumeId: DEMO_VOLUME_ID,
            volumeName: MAIN_VOLUME[platform].name,
            volumeRoot: '/',
            reach: { kind: 'ready' },
          }
        : null,
    rootLabel: null,
    mountAccount: null,
  }
}

/** The main volume's row for a visitor on `platform`. */
function mainVolumeOf(platform: Platform): LocationInfo {
  return location(platform, DEMO_VOLUME_ID, MAIN_VOLUME[platform].name, '/', 'main_volume')
}

/** The volume list for a visitor on `platform`. Exported for its test. */
export function demoLocations(platform: Platform): LocationInfo[] {
  return [
    location(platform, 'fav-home', 'you', DEMO_HOME, 'favorite', 'h'),
    location(platform, 'fav-desktop', 'Desktop', `${DEMO_HOME}/Desktop`, 'favorite', 'd'),
    location(platform, 'fav-documents', 'Documents', `${DEMO_HOME}/Documents`, 'favorite', 'o'),
    location(platform, 'fav-downloads', 'Downloads', `${DEMO_HOME}/Downloads`, 'favorite', 'w'),
    mainVolumeOf(platform),
  ]
}

const visitor = (): Platform => visitorPlatform(navigator.userAgent)

/** A path as the demo volume spells it: `~` expanded, no trailing slash. */
export function demoPath(raw: string): string {
  const expanded = raw === '~' || raw.startsWith('~/') ? DEMO_HOME + raw.slice(1) : raw
  return expanded.length > 1 && expanded.endsWith('/') ? expanded.slice(0, -1) : expanded
}

export const volumeAnswers = {
  listVolumes: () => ({ data: demoLocations(visitor()), timedOut: false }),
  getDefaultVolumeId: () => DEMO_VOLUME_ID,
  getE2eStartPath: () => null,
  resolvePathVolume: () => ({ volume: mainVolumeOf(visitor()), timedOut: false }),
  // The pane boot asks this for every restored tab. The one demo volume never moves and its
  // "registry" is complete from the start, so the tab stays put. Unanswered, the call rejects, the
  // app reads that as the startup sweep still running, and every tab opens unreachable.
  liveVolumePath: () => ({ kind: 'keep' }),
  // Every path is on the one volume, as `resolvePathVolume` says: the navigation edges (a search
  // result's Go to file, ⌘G) route through this.
  resolveLocation: (p) => ({
    location: { volumeId: DEMO_VOLUME_ID, path: demoPath(String(p.path)) },
    timedOut: false,
  }),
  // `pathExists` and `statPathsKinds` are the engine's: the volume they ask changes with every write.
  // Nothing on the demo volume syncs, and it stores every path as given.
  getSyncStatus: () => ({ data: {}, timedOut: false }),
  storedSpellings: (p) => ({ data: p.paths as string[], timedOut: false }),
} satisfies AnswerTable
