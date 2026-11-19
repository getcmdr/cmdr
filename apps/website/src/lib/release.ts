import latestRelease from '../../public/latest.json'
import apiServerWranglerToml from '../../../api-server/wrangler.toml?raw'
import { offeredPlatforms, parsePublicDownloadOses, type Os, type PlatformDownloads } from './download-platforms'
import { macDmgFile, macDmgSizes, type ReleaseDownload, type UpdateManifest } from './release-downloads'

// The release pipeline writes this file (`docs/guides/releasing.md` § The release asset manifest),
// so its shape is trusted, not validated.
const manifest: UpdateManifest = latestRelease

export const version = manifest.version

/** Display version: drops a trailing `.0` patch (`0.16.0` → `0.16`), keeps non-zero patches (`0.16.1`). */
export const versionDisplay = version.endsWith('.0') ? version.slice(0, -2) : version

const downloadBase = import.meta.env.PUBLIC_DOWNLOAD_BASE_URL
// The EU mirror, which holds every release the live manifest can name (`docs/guides/releasing.md`
// § Where downloads are served from). The frozen dev copy of `latest.json` names an older version
// it doesn't hold, so these links 404 in dev only.
const mirrorBase = `https://download.getcmdr.com/v${version}`

function dmgUrl(arch: string): string {
  // `?src=website` lets the download endpoint tag these as website-button downloads, distinct from
  // Homebrew (detected by User-Agent) and direct links shared elsewhere. The direct mirror link (used
  // when no download base is configured) carries no telemetry, so the param is irrelevant there.
  if (downloadBase) return `${downloadBase}/download/${version}/${arch}?src=website`
  return `${mirrorBase}/${macDmgFile(manifest, arch)}`
}

export const dmgUrls = {
  aarch64: dmgUrl('aarch64'),
  x86_64: dmgUrl('x86_64'),
  universal: dmgUrl('universal'),
}

/** A release file's link: the API server's route (a macOS DMG keeps its `/:version/:arch` alias), else the mirror. */
function downloadUrl(download: ReleaseDownload): string {
  if (download.os === 'macos' && download.kind === 'dmg') return dmgUrl(download.arch)
  if (downloadBase) {
    return `${downloadBase}/download/${version}/${download.os}/${download.arch}?kind=${download.kind}&src=website`
  }
  return `${mirrorBase}/${download.file}`
}

/**
 * The OSes whose downloads the API server serves: `PUBLIC_DOWNLOAD_OSES` in its `wrangler.toml`, read at build time
 * (the Docker build copies the file in). Adding `windows` there is the one switch, for the site as for the API.
 */
export const publicDownloadOses = parsePublicDownloadOses(apiServerWranglerToml)

/** What every download button and the download section offer, OS by OS (`src/lib/download-platforms.ts`). */
export const platformDownloads: PlatformDownloads[] = offeredPlatforms(manifest, publicDownloadOses, downloadUrl)

/** The offered OSes, macOS first. */
export const offeredOses: Os[] = platformDownloads.map((p) => p.os)

/**
 * Whether the site offers a Windows download. Pages hold Windows-only copy back until it does (`/trust#windows`), so
 * a deploy from `main` can't describe a Windows app nobody can get.
 */
export const offersWindows = offeredOses.includes('windows')

/**
 * The llms files' link lines for every offered OS but macOS (whose DMG lines they write out), each led by a newline,
 * so a page appends it to its last macOS line; empty while macOS is the one offered.
 */
export const otherOsDownloadLines = platformDownloads
  .filter((p) => p.os !== 'macos')
  .flatMap((p) =>
    p.options.map(
      (o) =>
        `\n- [Download for ${p.name} (${o.label})](${o.url}): ${p.name} ${o.kind === 'nsis' ? 'installer' : `${o.kind === 'deb' ? '.deb package' : 'AppImage'}`}`,
    ),
  )
  .join('')

/** Formatted download sizes (for example, "15 MB"), null if not yet populated by CI */
export const dmgSizes = macDmgSizes(manifest)
