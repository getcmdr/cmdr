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
  windowMaterial: true,
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
 * which ends File with Quit, so Quit is a palette command too, as on their desktop. A page can't
 * quit its tab: the bar greys Quit and the palette's says so (`answers.ts`'s `quitApp`). No
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
 * visitor gets the macOS bar and everyone else their OS's in-window bar (D5). Right-click menus stay
 * on the macOS value for everyone: the demo answers each `show_*` popup itself (`context-menu.ts`)
 * and has no `open_context_menu_in_page`, so a Windows visitor's `inPage` row would open no menu at
 * all. Exported for its test.
 */
export function visitorConventions(platform: Platform): Readonly<PlatformConventions> {
  return VISITOR_CONVENTIONS[platform]
}

function demoConventions(row: Readonly<PlatformConventions>): Readonly<PlatformConventions> {
  return Object.freeze({ ...row, contextMenus: CONVENTIONS_ON.macos.contextMenus })
}

/** One frozen object per platform, so every frame of the page reads the same one. */
const VISITOR_CONVENTIONS: Readonly<Record<Platform, Readonly<PlatformConventions>>> = {
  macos: demoConventions(CONVENTIONS_ON.macos),
  windows: demoConventions(CONVENTIONS_ON.windows),
  linux: demoConventions(CONVENTIONS_ON.otherLinux),
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
