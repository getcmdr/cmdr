/**
 * Which OSes the site offers a download for, and what each offers. One rule, so no page can link a build that isn't
 * out: an OS is offered when the API server serves its downloads (`PUBLIC_DOWNLOAD_OSES` in
 * `apps/api-server/wrangler.toml`, the switch that makes an OS's download public) AND the published release carries
 * a file of it (`latest.json`'s `downloads`, which the release pipeline writes from `ShippedOSes`). macOS is always
 * public, as on the API server. `apps/website/DETAILS.md` § Download platforms.
 *
 * Pure: no `import.meta.env`, no `?raw` imports, so the Astro config loader (`src/plugins/download-dropdown.ts`) and
 * the unit tests (`e2e/download-platforms.spec.ts`) can load it too. `release.ts` feeds it the real inputs.
 *
 * The functions under "In the browser" run in the visitor's browser: `Layout.astro` inlines them with `String(fn)`, so
 * each must stay self-contained (no imports, no module-level names, no TS-only runtime helpers). The unit tests
 * rebuild each from its source text to hold that.
 */
import { formatDownloadSize, releaseDownloads, type ReleaseDownload, type UpdateManifest } from './release-downloads'

export type Os = 'macos' | 'windows' | 'linux'

/** Every OS the site knows, in the order it lists them. */
export const allOses: readonly Os[] = ['macos', 'windows', 'linux']

export const osNames: Record<Os, string> = { macos: 'macOS', windows: 'Windows', linux: 'Linux' }

/**
 * The kinds of file the site offers per OS, the main one first. Windows' MSIX is the Microsoft Store's package, so
 * the site links the Store for it (task 57) and offers only the NSIS installer itself.
 */
const websiteKinds: Record<Os, readonly string[]> = {
  macos: ['dmg'],
  windows: ['nsis'],
  linux: ['appimage', 'deb'],
}

/** The arch a button offers before detection, or when detection finds nothing. Universal runs on every Mac. */
const defaultArches: Record<Os, string> = { macos: 'universal', windows: 'x86_64', linux: 'x86_64' }

/** The arch order inside one kind, for the dropdown. */
const archOrder = ['aarch64', 'x86_64', 'universal']

function isOs(value: string): value is Os {
  return (allOses as readonly string[]).includes(value)
}

/**
 * The OSes the API server serves downloads for, read off `wrangler.toml`'s `PUBLIC_DOWNLOAD_OSES` line the way
 * `publicDownloadOses` in `apps/api-server/src/telemetry/download.ts` reads the var: macOS always, plus every known OS
 * the comma-separated list names. Throws unless the file has exactly one `PUBLIC_DOWNLOAD_OSES = "…"` line: a moved,
 * single-quoted, or multi-line value would leave the API serving an OS the site never offers, so the build stops.
 */
export function parsePublicDownloadOses(wranglerToml: string): ReadonlySet<Os> {
  const lines = wranglerToml.split('\n').filter((l) => /^\s*PUBLIC_DOWNLOAD_OSES\s*=/.test(l))
  const value = lines.length === 1 ? /^\s*PUBLIC_DOWNLOAD_OSES\s*=\s*"([^"\n]*)"\s*(#.*)?$/.exec(lines[0]) : null
  if (!value) {
    throw new Error(
      'apps/api-server/wrangler.toml needs one `PUBLIC_DOWNLOAD_OSES = "macos,…"` line: the site reads it to know which OSes to offer',
    )
  }
  const listed = value[1].split(',').map((os) => os.trim())
  return new Set<Os>(['macos', ...listed.filter(isOs)])
}

/** One file a download button or dropdown offers. */
export interface DownloadOption {
  arch: string
  kind: string
  /** The dropdown's name for it, like "Apple Silicon" or "AppImage · ARM64". */
  label: string
  url: string
  /** Like "15 MB", or null when the manifest has no size (a dev copy from before CI filled them). */
  size: string | null
}

/** One OS's downloads. */
export interface PlatformDownloads {
  os: Os
  name: string
  defaultArch: string
  options: DownloadOption[]
}

/** One name per arch off macOS, the same on Windows and Linux. */
function archName(arch: string): string {
  return arch === 'aarch64' ? 'ARM64' : arch === 'x86_64' ? 'x64' : arch
}

function optionLabel(os: Os, kind: string, arch: string): string {
  if (os === 'macos') return arch === 'aarch64' ? 'Apple Silicon' : arch === 'x86_64' ? 'Intel' : 'Universal'
  if (os === 'windows') return `Installer · ${archName(arch)}`
  return `${kind === 'deb' ? '.deb' : 'AppImage'} · ${archName(arch)}`
}

function isReleaseDownload(d: unknown): boolean {
  const e = d as Record<string, unknown> | null
  return ['os', 'arch', 'kind', 'file'].every((k) => typeof e?.[k] === 'string') && typeof e?.size === 'number'
}

/**
 * The OSes the site offers, in `allOses` order, each with its files. `urlFor` turns a release file into its link
 * (`release.ts` points it at the API server's download route, or the mirror in dev).
 *
 * Throws on a malformed `downloads` list, and unless macOS (always public) has its universal DMG: that's every
 * button's link with JS off and for a Mac whose arch is unknown, so without it the build would ship buttons with no
 * link, or an Apple silicon DMG for Intel Macs. A failed build keeps the deployed site up.
 */
export function offeredPlatforms(
  manifest: UpdateManifest,
  publicOses: ReadonlySet<Os>,
  urlFor: (download: ReleaseDownload) => string,
): PlatformDownloads[] {
  const raw: unknown = manifest.downloads
  if (raw !== undefined && !(Array.isArray(raw) && raw.every(isReleaseDownload))) {
    throw new Error(`latest.json ${manifest.version} has a malformed \`downloads\` list, so the site won't build`)
  }
  const downloads = releaseDownloads(manifest)
  if (!downloads.some((d) => d.os === 'macos' && d.kind === 'dmg' && d.arch === 'universal')) {
    throw new Error(`latest.json ${manifest.version} has no universal macOS DMG, so the site won't build`)
  }
  return allOses.flatMap((os) => {
    if (!publicOses.has(os)) return []
    const kinds = websiteKinds[os]
    const files = downloads
      .filter((d) => d.os === os && kinds.includes(d.kind))
      .sort(
        (a, b) =>
          kinds.indexOf(a.kind) - kinds.indexOf(b.kind) || archOrder.indexOf(a.arch) - archOrder.indexOf(b.arch),
      )
    if (files.length === 0) return []
    const options = files.map((d) => ({
      arch: d.arch,
      kind: d.kind,
      label: optionLabel(os, d.kind, d.arch),
      url: urlFor(d),
      size: d.size > 0 ? formatDownloadSize(d.size) : null,
    }))
    const defaultArch = options.some((o) => o.arch === defaultArches[os]) ? defaultArches[os] : options[0].arch
    return [{ os, name: osNames[os], defaultArch, options }]
  })
}

/** "macOS", "macOS and Windows", "macOS, Windows, and Linux". */
export function formatOsList(oses: readonly Os[]): string {
  const names = oses.map((os) => osNames[os])
  if (names.length <= 2) return names.join(' and ')
  return `${names.slice(0, -1).join(', ')}, and ${names[names.length - 1]}`
}

/**
 * The download section's newsletter line: what isn't out yet, then the ask. The form is the general newsletter
 * (about once a month, as the download popup says), so the line promises only that the news will be in it, never a
 * mail on release day. Linux has an alpha people build from source, so it says so while Linux isn't offered.
 */
export function newsletterLabel(offered: readonly Os[]): string {
  const windows = !offered.includes('windows')
  const linux = !offered.includes('linux')
  const cadence = 'The newsletter, about once a month,'
  if (windows && linux) {
    return `Windows is on its way, and Linux is in alpha (self-build for now). ${cadence} will announce both:`
  }
  if (windows) return `Windows is on its way. ${cadence} will announce it:`
  if (linux) return `Linux is in alpha (self-build for now). ${cadence} will announce its download:`
  return 'New versions go out in the newsletter, about once a month:'
}

/** The note a visitor gets whose device isn't one Cmdr runs on, or that detection couldn't place. Neutral on purpose. */
export function otherDeviceNote(offered: readonly Os[]): string {
  return offered.length === 1
    ? `Cmdr is a desktop app for ${formatOsList(offered)}.`
    : `Cmdr is a desktop app for ${formatOsList(offered)}. Pick yours below.`
}

// In the browser. Self-contained functions only, see the header.

/** The parts of `navigator` detection reads, all optional, since browsers differ in what they expose. */
export interface VisitorNavigator {
  userAgent?: string
  platform?: string
  maxTouchPoints?: number
  userAgentData?: { platform?: string; mobile?: boolean } | null
}

/**
 * The visitor's desktop OS, or null for a phone, a tablet, a Chromebook, or anything unknown. It runs on the
 * visitor's device and its answer never leaves the page. Order: User-Agent Client Hints' platform (Chromium; the
 * most direct answer), then the User-Agent string, then the old `navigator.platform`. Android and ChromeOS say
 * "Linux" in their user agents, and iPadOS Safari and Android tablets in "desktop site" mode say "Macintosh" or
 * "X11; Linux", so those are ruled out first: without Client Hints, a Mac or Linux user agent on a touch screen
 * counts as unknown (a touchscreen Linux laptop in Firefox lands there too, and gets the neutral note).
 */
export function detectVisitorOs(nav: VisitorNavigator): Os | null {
  // Per OS: its Client Hints platform, how its user agent says it, and how `navigator.platform` does.
  const rules: [string, RegExp, RegExp, Os][] = [
    ['Windows', /Windows/, /^Win/, 'windows'],
    ['macOS', /Macintosh|Mac OS X/, /^Mac/, 'macos'],
    ['Linux', /Linux|X11/, /Linux/, 'linux'],
  ]
  const hints = nav.userAgentData
  if (hints && hints.platform) {
    const byHints = rules.find((r) => r[0] === hints.platform)
    return byHints && !hints.mobile ? byHints[3] : null
  }
  const ua = nav.userAgent || ''
  if (/Android|iPhone|iPad|iPod|CrOS|Mobile/.test(ua)) return null
  if (/Macintosh|Mac OS X|Linux|X11/.test(ua) && (nav.maxTouchPoints || 0) > 1) return null
  const platform = nav.platform || ''
  const match = rules.find((r) => r[1].test(ua)) || rules.find((r) => r[2].test(platform))
  return match ? match[3] : null
}

/**
 * The CPU arch a Windows or Linux user agent names, or null when it names none. Chromium freezes its user agent to
 * x86_64 everywhere, so the async Client Hints answer (`Layout.astro`) refines this when it can; Firefox on Linux
 * still names the real one. `armv8l` is a 32-bit userland, so it names no arch Cmdr builds. Macs get their arch from
 * WebGL instead (`macArchFromRenderer`).
 */
export function detectVisitorArch(nav: VisitorNavigator): string | null {
  const text = (nav.userAgent || '') + ' ' + (nav.platform || '')
  if (/aarch64|arm64|armv8(?!l)/i.test(text)) return 'aarch64'
  if (/x86_64|x64|amd64|Win64|WOW64/i.test(text)) return 'x86_64'
  return null
}

/**
 * A Mac's arch from its WebGL renderer string (`UNMASKED_RENDERER_WEBGL`), or null when it doesn't say. Chrome and
 * Firefox name the GPU ("ANGLE (Apple, ANGLE Metal Renderer: Apple M2, …)" or an Intel or AMD part). Safari reports
 * "Apple GPU" on every Mac, Intel ones included, to resist fingerprinting (ua-parser-js#732, and an Intel Mac in
 * forum.babylonjs.com/t/63890, read 2026-11-19), so that string means unknown and the button stays universal.
 */
export function macArchFromRenderer(renderer: string): string | null {
  if (/Apple M\d/.test(renderer)) return 'aarch64'
  if (/Intel|AMD|Radeon/.test(renderer)) return 'x86_64'
  return null
}

/** What one download button shows: the OS whose files it offers, and the file its main link points at. */
export interface ButtonChoice {
  os: string
  option: DownloadOption
}

/**
 * The file a download button's main link offers. A button pinned to an OS (a card in the download section) keeps
 * it; any other button follows the visitor's OS when the site offers it, else macOS, the one OS always offered.
 * Within the visitor's own OS, their arch when there's a build for it; otherwise the OS's default (Windows on ARM runs
 * the x64 build, and a Windows visitor offered macOS gets the universal DMG, which runs on every Mac). Only the main kind counts (the AppImage, never the `.deb`). Null when the button offers nothing.
 */
export function chooseDownload(
  platforms: Record<string, Pick<PlatformDownloads, 'defaultArch' | 'options'> | undefined>,
  pick: { visitorOs: string | null; arch: string | null; pinnedOs: string | null },
): ButtonChoice | null {
  const os = pick.pinnedOs || (pick.visitorOs && platforms[pick.visitorOs] ? pick.visitorOs : 'macos')
  const platform = platforms[os]
  if (!platform || platform.options.length === 0) return null
  const mainKind = platform.options[0].kind
  const main = platform.options.filter((o) => o.kind === mainKind)
  // The visitor's arch says nothing about another OS's build: a Windows PC's x64 isn't a reason for the Intel DMG.
  const wanted = os === pick.visitorOs ? pick.arch : null
  const option = main.find((o) => o.arch === wanted) || main.find((o) => o.arch === platform.defaultArch) || main[0]
  return { os: os, option: option }
}
