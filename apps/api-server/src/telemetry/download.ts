import { Hono, type Context } from 'hono'
import { isClientOs, type ClientOs } from '../client-os'
import { callerIp, hashCallerIp, type Bindings } from '../types'
import { classifyUaFamily } from '../user-agent'
import { todayUtc, versionPattern } from './telemetry'

/**
 * `/download`: redirects to a release file and logs the hit (version, OS, arch, kind, country, source, attribution),
 * which the dashboard's acquisition numbers read. `DETAILS.md` § Download tracking.
 */
const download = new Hono<{ Bindings: Bindings }>()

/** The archs each OS's builds come in, in the repo's spelling (`x86_64`, never the filenames' `x64`). */
const archesByOs: Record<ClientOs, ReadonlySet<string>> = {
  macos: new Set(['aarch64', 'x86_64', 'universal']),
  windows: new Set(['x86_64', 'aarch64']),
  linux: new Set(['x86_64', 'aarch64']),
}

/**
 * The kinds of file people download per OS, the default first: `downloadKinds` in
 * `scripts/release-assets/manifest/manifest.go`, split by the OS each kind belongs to.
 */
const kindsByOs: Record<ClientOs, readonly string[]> = {
  macos: ['dmg', 'pkg'],
  windows: ['nsis', 'msix'],
  linux: ['appimage', 'deb'],
}

/**
 * The OSes whose downloads this route serves, from the `PUBLIC_DOWNLOAD_OSES` var (comma-separated, `wrangler.toml`).
 * Absent means macOS only, so a Worker deployed without the var can't expose a build nobody announced. A Windows or
 * Linux file a release already carries stays unreachable here until its OS joins the list.
 */
export function publicDownloadOses(env: Pick<Bindings, 'PUBLIC_DOWNLOAD_OSES'>): ReadonlySet<ClientOs> {
  const listed = (env.PUBLIC_DOWNLOAD_OSES ?? 'macos').split(',').map((os) => os.trim())
  return new Set(listed.filter(isClientOs))
}

// Link-unfurl bots and crawlers fetch the download URL to build a preview (Discord, Slack, etc.) or
// to index it, which inflates the count with hits that aren't real installs. We still serve them the
// 302 (no point breaking a preview), but we skip the D1 row so they never reach the dashboard. The
// match is on the User-Agent, so it's a heuristic lower bound, not a guarantee; a spoofed UA slips
// through, but the common unfurlers below all send these identifiable strings.
const botUserAgentPattern =
  /bot|crawl|spider|slurp|preview|unfurl|facebookexternalhit|embedly|whatsapp|telegram|discord|slack|twitter|linkedin|pinterest|curl|wget|python-requests|go-http-client|headlesschrome|monitor|uptime|pingdom/i

function isBotUserAgent(userAgent: string | undefined): boolean {
  if (!userAgent) return true // No UA at all is bot-like; real browsers and Homebrew always send one.
  // Homebrew downloads via curl, so it'd match the `curl` rule below, but it's a real install path.
  if (/homebrew/i.test(userAgent)) return false
  return botUserAgentPattern.test(userAgent)
}

/**
 * Where a download came from, for the dashboard's "new installs by source" breakdown.
 * - `homebrew`: the Homebrew cask, which fetches via curl with a `Homebrew/x.y.z` User-Agent.
 * - `website`: the getcmdr.com download button, which appends `?src=website`.
 * - `other`: everything else (direct links shared in chats, the README, etc.).
 */
function classifyDownloadSource(userAgent: string | undefined, srcParam: string | undefined): string {
  if (userAgent && /homebrew/i.test(userAgent)) return 'homebrew'
  if (srcParam === 'website') return 'website'
  return 'other'
}

const maxRefLength = 120
const disallowedRefChars = /[^a-z0-9._:-]/g

/**
 * Sanitize the `?ref` first-touch channel value for storage. The website computes it client-side
 * (a UTM source/campaign, a referrer hostname, or `direct`) and appends it to the download URL, but
 * we never trust client input: lowercase, drop anything outside `[a-z0-9._:-]`, and cap at 120 chars.
 * Returns `null` when absent or when nothing survives sanitizing, so the column stays NULL rather
 * than holding an empty string. Mirrors the website's normalization so both ends agree.
 */
function sanitizeRef(refParam: string | undefined): string | null {
  if (!refParam) return null
  const cleaned = refParam.toLowerCase().replace(disallowedRefChars, '').slice(0, maxRefLength)
  return cleaned.length > 0 ? cleaned : null
}

const disallowedHostChars = /[^a-z0-9.-]/g

/**
 * Sanitize the HTTP `Referer` header down to a storable host. A direct hit to `/download` (a link shared
 * on AlternativeTo, a directory, GitHub, Reddit, a forum) carries no `?ref` but does carry a `Referer`,
 * so its host is the first-party signal for where that download came from. We keep the HOST only (never
 * the path or query, so a referring page's own query string can't leak in), lowercase it, strip a leading
 * `www.`, drop anything outside `[a-z0-9.-]`, and cap at 120 chars. Returns `null` when the header is
 * absent, unparseable, or sanitizes to empty, so the column stays NULL (the `(none)` bucket).
 */
function sanitizeRefererHost(refererHeader: string | undefined): string | null {
  if (!refererHeader) return null
  let host: string
  try {
    host = new URL(refererHeader).hostname
  } catch {
    return null
  }
  const cleaned = host
    .toLowerCase()
    .replace(/^www\./, '')
    .replace(disallowedHostChars, '')
    .slice(0, maxRefLength)
  return cleaned.length > 0 ? cleaned : null
}

const maxUserAgentLength = 400

/** Cap the stored User-Agent so a hostile client can't bloat a row; `null` when absent. */
function cappedUserAgent(userAgent: string | undefined): string | null {
  if (!userAgent) return null
  return userAgent.slice(0, maxUserAgentLength)
}

const latestManifestUrl = 'https://getcmdr.com/latest.json'
const githubLatestReleaseUrl = 'https://api.github.com/repos/getcmdr/cmdr/releases/latest'
const releasesPageUrl = 'https://github.com/getcmdr/cmdr/releases/latest'

/** Five minutes of edge caching: a download burst costs one origin fetch, and a release goes live fast. */
const latestVersionCacheSeconds = 300

/** A published release's `latest.json` doesn't change after the release, so a day of edge caching is safe. */
const releaseManifestCacheSeconds = 86_400

/** One entry of `latest.json`'s `downloads` (`scripts/release-assets/DETAILS.md` § `latest.json`). */
interface ReleaseDownload {
  os: string
  arch: string
  kind: string
  file: string
}

/**
 * What a release filename may look like before it goes into a redirect URL. The manifest names every file
 * `Cmdr_<version>_<arch>…`; anything else (a slash, a query) would be a manifest we shouldn't trust.
 */
const releaseFilePattern = /^Cmdr_[0-9A-Za-z._-]+$/

/** The manifest's `downloads`, keeping only well-formed entries; null when there's no list at all. */
function parseReleaseDownloads(value: unknown): ReleaseDownload[] | null {
  if (!Array.isArray(value)) return null
  return value.filter(
    (entry): entry is ReleaseDownload =>
      typeof entry === 'object' &&
      entry !== null &&
      ['os', 'arch', 'kind', 'file'].every((key) => typeof (entry as Record<string, unknown>)[key] === 'string') &&
      releaseFilePattern.test((entry as ReleaseDownload).file),
  )
}

/** A version the release sources named, plus its downloads when the source carried them. */
interface ResolvedRelease {
  version: string
  downloads: ReleaseDownload[] | null
}

/**
 * Resolve the `latest` version token to a concrete version number (and, from our own manifest, its downloads), or
 * `null` when both sources fail.
 *
 * Primary is our own release manifest, the same file the in-app updater reads, so `latest` can never
 * name a version the updater doesn't know. GitHub's release API is the fallback for the window where
 * the website is down or mid-deploy; it's rate-limited unauthenticated, which is fine for a fallback.
 * Both answers are validated against `versionPattern` before use: this string goes into a redirect URL
 * and a D1 row.
 */
async function resolveLatestRelease(): Promise<ResolvedRelease | null> {
  try {
    const res = await fetch(latestManifestUrl, {
      cf: { cacheTtl: latestVersionCacheSeconds, cacheEverything: true },
    })
    if (res.ok) {
      const manifest = await res.json<{ version?: unknown; downloads?: unknown }>()
      if (typeof manifest.version === 'string' && versionPattern.test(manifest.version)) {
        return { version: manifest.version, downloads: parseReleaseDownloads(manifest.downloads) }
      }
    }
  } catch {
    // Website unreachable; fall through to GitHub.
  }

  try {
    const res = await fetch(githubLatestReleaseUrl, {
      // GitHub 403s an API request without a User-Agent.
      headers: { 'user-agent': 'cmdr-api-server', accept: 'application/vnd.github+json' },
      cf: { cacheTtl: latestVersionCacheSeconds, cacheEverything: true },
    })
    if (res.ok) {
      const release = await res.json<{ tag_name?: unknown }>()
      if (typeof release.tag_name === 'string') {
        const tagged = release.tag_name.replace(/^v/, '')
        if (versionPattern.test(tagged)) return { version: tagged, downloads: null }
      }
    }
  } catch {
    // Both sources down.
  }

  return null
}

/**
 * A release's downloads, read from the `latest.json` its GitHub release carries (the mirror holds no manifest), or
 * null when it can't be read or predates the `downloads` list.
 */
async function releaseDownloadsOf(version: string): Promise<ReleaseDownload[] | null> {
  try {
    const res = await fetch(`https://github.com/getcmdr/cmdr/releases/download/v${version}/latest.json`, {
      cf: { cacheTtl: releaseManifestCacheSeconds, cacheEverything: true },
    })
    if (!res.ok) return null
    const manifest = await res.json<{ downloads?: unknown }>()
    return parseReleaseDownloads(manifest.downloads)
  } catch {
    return null
  }
}

/**
 * The filename of one download in a release, or null when the release carries no such file. A macOS DMG is named
 * without a lookup, as it always was: every release has the three, and their names never moved (tauri-action spells
 * the Intel one `_x64.dmg`). Everything else comes from the release's own `downloads` list, because filenames are
 * data (`scripts/release-assets/DETAILS.md`): Tauri's bundler names each OS's files its own way.
 */
async function releaseFileName(release: ResolvedRelease, os: ClientOs, arch: string, kind: string) {
  if (os === 'macos' && kind === 'dmg') {
    return `Cmdr_${release.version}_${arch === 'x86_64' ? 'x64' : arch}.dmg`
  }
  const downloads = release.downloads ?? (await releaseDownloadsOf(release.version))
  return downloads?.find((d) => d.os === os && d.arch === arch && d.kind === kind)?.file ?? null
}

// The SHA-256 list for a release's downloads, so someone who downloaded from the website can check what
// they got. Registered BEFORE `/:version/:arch` so the static `checksums` segment wins over the
// param; `checksums` is deliberately NOT an arch, which answers a different question and is read by
// the D1 breakdown.
//
// ❌ Writes no `downloads` row. A checksum fetch is not an app download, and counting one would
// inflate the per-version numbers with people who never took a DMG. That's also why the website
// links it without the `data-download-link` attribution hook.
download.get('/download/:version/checksums', async (c) => {
  const requestedVersion = c.req.param('version')
  const wantsLatest = requestedVersion === 'latest'

  if (!wantsLatest && !versionPattern.test(requestedVersion)) {
    return c.json({ error: 'Invalid version' }, 400)
  }

  const version = wantsLatest ? (await resolveLatestRelease())?.version : requestedVersion
  if (!version) {
    // Same fallback as the download redirect: the releases page lists every asset, `checksums.txt`
    // included, so the visitor can still get there by hand.
    return c.redirect(releasesPageUrl, 302)
  }

  return c.redirect(releaseFileUrl(version, 'checksums.txt'), 302)
})

/**
 * The first release the EU mirror holds (backfilled on 2026-11-05; the release pipeline's `publish`
 * job mirrors every later one before it goes Latest). GitHub Releases keeps every version.
 */
const firstMirroredVersion = [0, 59, 0] as const

/** `version` is a validated `X.Y.Z` (`versionPattern`), so a plain numeric compare is exact. */
function isMirrored(version: string): boolean {
  const parts = version.split('.').map(Number)
  for (let i = 0; i < firstMirroredVersion.length; i++) {
    if (parts[i] !== firstMirroredVersion[i]) return parts[i] > firstMirroredVersion[i]
  }
  return true
}

/**
 * Where a release file downloads from: the EU R2 mirror behind `download.getcmdr.com` for every
 * version it holds, else the GitHub release. `docs/guides/releasing.md` § Where downloads are served from.
 */
function releaseFileUrl(version: string, file: string): string {
  return isMirrored(version)
    ? `https://download.getcmdr.com/v${version}/${file}`
    : `https://github.com/getcmdr/cmdr/releases/download/v${version}/${file}`
}

/** One `/download` request, its params as they came in (validated by `serveDownload`). */
interface DownloadRequest {
  version: string
  os: string
  arch: string
  /** `?kind=`, or undefined for the OS's default kind. */
  kind: string | undefined
}

/** A `/download` request whose params all check out. */
interface ValidDownloadRequest {
  wantsLatest: boolean
  os: ClientOs
  arch: string
  kind: string
}

/** The request with its params checked against the OS's archs and kinds, or the 400's message. */
function parseDownloadRequest(request: DownloadRequest): ValidDownloadRequest | string {
  const wantsLatest = request.version === 'latest'
  const { os, arch } = request
  if ((!wantsLatest && !versionPattern.test(request.version)) || !isClientOs(os) || !archesByOs[os].has(arch)) {
    return 'Invalid version, OS, or architecture'
  }
  const kind = request.kind ?? kindsByOs[os][0]
  if (!kindsByOs[os].includes(kind)) return 'Invalid kind for this OS'
  return { wantsLatest, os, arch, kind }
}

/**
 * The shared body of both `/download` routes: validate, resolve the version and the file, redirect, and log the hit.
 * Order matters: the public-OS gate answers before any release lookup, and nothing is logged for a request that
 * doesn't end in a real file's redirect.
 */
async function serveDownload(c: Context<{ Bindings: Bindings }>, request: DownloadRequest): Promise<Response> {
  const parsed = parseDownloadRequest(request)
  if (typeof parsed === 'string') return c.json({ error: parsed }, 400)
  const { wantsLatest, os, arch, kind } = parsed
  if (!publicDownloadOses(c.env).has(os)) {
    return c.json({ error: 'No downloads for this OS yet' }, 404)
  }

  // `latest` keeps published links (app directories, the README, blog posts, anything we can't edit
  // per release) pointing at the current build. It resolves to a concrete version here, so the redirect
  // names a real asset and D1 keeps its per-version breakdown instead of a `latest` bucket.
  const release = wantsLatest ? await resolveLatestRelease() : { version: request.version, downloads: null }
  if (!release) {
    // Neither release source answered. Send the visitor to the releases page, where they can pick a
    // build by hand, rather than to a URL we know is dead. Nothing to log: there's no version to
    // attribute the download to, and a guessed one would corrupt the per-version counts.
    return c.redirect(releasesPageUrl, 302)
  }
  const file = await releaseFileName(release, os, arch, kind)
  if (!file) {
    return c.json({ error: 'This release has no such download' }, 404)
  }
  const redirectUrl = releaseFileUrl(release.version, file)

  // Drop bot/unfurler hits before they reach D1, but still serve the file.
  const userAgent = c.req.header('user-agent')
  if (isBotUserAgent(userAgent)) {
    return c.redirect(redirectUrl, 302)
  }

  const cf = c.req.raw.cf as { country?: string; continent?: string } | undefined
  const country = cf?.country ?? 'unknown'
  const continent = cf?.continent ?? 'unknown'
  const source = classifyDownloadSource(userAgent, c.req.query('src'))
  // First-touch channel the website captured on the visitor's first page view and forwarded here.
  const ref = sanitizeRef(c.req.query('ref'))
  // The hit's own `Referer` host + User-Agent: the only first-party signal for a direct `/download`
  // link (no `?ref`), so the dashboard can attribute the otherwise-unknown downloads. See migration 0010.
  const referer = sanitizeRefererHost(c.req.header('referer'))
  const storedUserAgent = cappedUserAgent(userAgent)
  // Classify at WRITE time as well as read time, so the install-plausibility signal outlives the raw
  // UA that the retention sweep clears after `DOWNLOAD_IDENTIFIER_RETENTION_DAYS`. Against the FILE's
  // OS: a Windows browser fetching the installer is a real install, fetching a DMG it isn't.
  const uaFamily = classifyUaFamily(userAgent, os)

  // Peppered, daily-salted IP hash: lets the dashboard count distinct same-day downloaders
  // (`COUNT(DISTINCT hashed_ip)`) without storing a recoverable address or anything linkable across
  // days. Same scheme as `/update-check`; see `hashCallerIp` for why both parts matter.
  const hashedIp = await hashCallerIp(callerIp(c.req), todayUtc(), c.env.IP_HASH_PEPPER)

  // Write to D1 (fire-and-forget). One row per request: the raw count stays `COUNT(*)`.
  const dbWrite = c.env.TELEMETRY_DB.prepare(
    `INSERT INTO downloads (app_version, arch, country, continent, hashed_ip, source, ref, referer, user_agent, ua_family, os, kind) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
  )
    .bind(
      release.version,
      arch,
      country,
      continent,
      hashedIp,
      source,
      ref,
      referer,
      storedUserAgent,
      uaFamily,
      os,
      kind,
    )
    .run()
    .catch(() => {})

  try {
    c.executionCtx.waitUntil(dbWrite)
  } catch {
    // executionCtx unavailable (for example, in tests); await inline as fallback
    await dbWrite
  }

  return c.redirect(redirectUrl, 302)
}

// The macOS DMG route every link before Windows names (the website, the README, app directories, the
// Homebrew cask's docs). It stays an alias of `/download/:version/macos/:arch` for good: those links
// can't be edited per release.
download.get('/download/:version/:arch', (c) => {
  const { version, arch } = c.req.param()
  return serveDownload(c, { version, os: 'macos', arch, kind: 'dmg' })
})

download.get('/download/:version/:os/:arch', (c) => {
  const { version, os, arch } = c.req.param()
  return serveDownload(c, { version, os, arch, kind: c.req.query('kind') })
})

export { download }
