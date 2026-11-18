import latestRelease from '../../public/latest.json'
import { macDmgFile, macDmgSizes, releaseShipsOs, type UpdateManifest } from './release-downloads'

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

/**
 * Whether the published release has a Windows download. Pages hold Windows-only copy back until it
 * does (`/trust#windows`), so a deploy from `main` can't describe a Windows app nobody can get.
 */
export const releaseShipsWindows = releaseShipsOs(manifest, 'windows')

/** Formatted download sizes (for example, "15 MB"), null if not yet populated by CI */
export const dmgSizes = macDmgSizes(manifest)
