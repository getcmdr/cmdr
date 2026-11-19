/**
 * The demo's stand-in for the backend's platform init script (`#lib/platform`). A page has no
 * backend to say which OS it runs on, so the demo answers for the visitor's own computer: the
 * shortcuts must match the keyboard under their hands, ⌘ on a Mac and Ctrl elsewhere. The
 * capabilities follow that answer. See `DETAILS.md` § Platform.
 */
import {
  CAPABILITIES_GLOBAL,
  CONVENTIONS_GLOBAL,
  CONVENTIONS_ON,
  NO_CAPABILITIES,
  PLATFORM_GLOBAL,
  type Platform,
  type PlatformCapabilities,
  type PlatformConventions,
} from '#lib/platform/index.js'

/** The visitor's OS, from the one signal a page has. Exported for its test. */
export function visitorPlatform(userAgent: string): Platform {
  if (/Macintosh|Mac OS X/.test(userAgent)) return 'macos'
  if (/Windows/.test(userAgent)) return 'windows'
  return 'linux'
}

/**
 * What the demo shows a Mac visitor: the Mac app's surfaces, as the desktop's macOS answer lists
 * them, so the demo looks like the app it advertises. Its fake backend answers the probes behind
 * them (Full Disk Access granted, `answers.ts`) or leaves them unanswered. Written out rather than
 * derived, so a new capability is a type error here until someone decides what the demo shows.
 */
const MAC_VISITOR_CAPABILITIES: Readonly<PlatformCapabilities> = Object.freeze({
  quickLook: true,
  getInfo: true,
  openTerminalHere: true,
  textEditorChoice: true,
  cloudFileActions: true,
  colorTags: true,
  appMenu: true,
  servicesMenu: true,
  fullDiskAccess: true,
  finderExtension: true,
  revealHandler: true,
  dockPin: true,
  cliCommand: true,
  overlayTitleBar: true,
  imageIndexing: true,
  localNetworkPermission: true,
  localAi: true,
  safeSaveFiles: true,
  appBundles: true,
  fullScreen: true,
  homebrewCask: true,
  globalShortcuts: true,
  // No surface asks it; `true` keeps this table the desktop's macOS answer.
  memoryDiagnostics: true,
  cpuDiagnostics: true,
})

/**
 * What every other visitor gets: no OS-backed surface, as the desktop answers for Windows and
 * Linux. `appMenu` stays off, since their bar is their own OS's (`crates/cmdr-listing-wasm/src/menu.rs`),
 * which ends File with Quit, so Quit is a palette command too, as on their desktop. No
 * `overlayTitleBar` either: their windows keep the OS's title bar above the page, which the demo
 * draws (`title-bar.ts`), so no page leaves room for buttons over it.
 */
const OTHER_VISITOR_CAPABILITIES: Readonly<PlatformCapabilities> = NO_CAPABILITIES

/** The demo's capabilities for a visitor on `platform`. Exported for its test. */
export function visitorCapabilities(platform: Platform): Readonly<PlatformCapabilities> {
  return platform === 'macos' ? MAC_VISITOR_CAPABILITIES : OTHER_VISITOR_CAPABILITIES
}

/**
 * The demo's conventions for a visitor on `platform`: the desktop app's own row for their OS
 * (desktop-neutral on Linux: a page can't tell GNOME from KDE), menu model included, so a Mac
 * visitor gets the macOS bar and everyone else their OS's in-window bar (D5). Exported for its test.
 */
export function visitorConventions(platform: Platform): Readonly<PlatformConventions> {
  return VISITOR_CONVENTIONS[platform]
}

/** One frozen object per platform, so every frame of the page reads the same one. */
const VISITOR_CONVENTIONS: Readonly<Record<Platform, Readonly<PlatformConventions>>> = {
  macos: Object.freeze({ ...CONVENTIONS_ON.macos }),
  windows: Object.freeze({ ...CONVENTIONS_ON.windows }),
  linux: Object.freeze({ ...CONVENTIONS_ON.otherLinux }),
}

/**
 * Defines the platform, its capabilities, and its conventions in `frame` before any app module
 * reads them, read-only as on desktop. The look (`data-look`: the menus' and the controls' tokens)
 * follows the platform by itself, so a Windows visitor's bar, menus, and controls wear Windows 11's.
 */
export function installPlatform(frame: Window): void {
  const platform = visitorPlatform(frame.navigator.userAgent)
  Object.defineProperty(frame, PLATFORM_GLOBAL, { value: platform })
  Object.defineProperty(frame, CAPABILITIES_GLOBAL, { value: visitorCapabilities(platform) })
  Object.defineProperty(frame, CONVENTIONS_GLOBAL, { value: visitorConventions(platform) })
}
