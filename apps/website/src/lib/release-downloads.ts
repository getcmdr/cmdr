/**
 * The release's downloads, read from the update manifest (`public/latest.json`). The release pipeline writes a
 * `downloads` list into it from the release asset manifest (`scripts/release-assets/manifest/manifest.go`): one entry
 * per file people download, with its OS, arch, kind, file name, and size. Manifests from before that list carry only
 * `dmgSizes`, so this falls back to the three macOS DMGs those always named.
 *
 * No `import.meta.env` here: `src/plugins/download-dropdown.ts` runs in Astro's config loader, which doesn't have the
 * `PUBLIC_*` vars, and shares this with `release.ts`.
 */

export interface ReleaseDownload {
  os: string
  arch: string
  kind: string
  file: string
  size: number
}

export interface UpdateManifest {
  version: string
  dmgSizes?: { aarch64: number; x86_64: number; universal: number }
  downloads?: ReleaseDownload[]
}

/** Every download the release carries; the macOS DMGs from `dmgSizes` for a manifest without `downloads`. */
export function releaseDownloads(manifest: UpdateManifest): ReleaseDownload[] {
  if (manifest.downloads) return manifest.downloads
  const sizes = manifest.dmgSizes
  if (!sizes) return []
  return (['aarch64', 'x86_64', 'universal'] as const).map((arch) => ({
    os: 'macos',
    arch,
    kind: 'dmg',
    // Release files spell the Intel arch `x64`, never `x86_64`.
    file: `Cmdr_${manifest.version}_${arch === 'x86_64' ? 'x64' : arch}.dmg`,
    size: sizes[arch],
  }))
}

/** One OS's downloads of one kind, by arch. */
export function downloadsByArch(manifest: UpdateManifest, os: string, kind: string): Map<string, ReleaseDownload> {
  return new Map(
    releaseDownloads(manifest)
      .filter((d) => d.os === os && d.kind === kind)
      .map((d) => [d.arch, d]),
  )
}

/** A size the way the download buttons show it, like "15 MB". */
export function formatDownloadSize(bytes: number): string {
  return `${String(Math.round(bytes / (1024 * 1024)))} MB`
}

/**
 * The formatted sizes of the three macOS DMGs, or null until the release has them all (a dev manifest from before CI
 * filled `dmgSizes` held zeros).
 */
export function macDmgSizes(manifest: UpdateManifest): { aarch64: string; x86_64: string; universal: string } | null {
  const dmgs = downloadsByArch(manifest, 'macos', 'dmg')
  const aarch64 = dmgs.get('aarch64')?.size ?? 0
  const x86_64 = dmgs.get('x86_64')?.size ?? 0
  const universal = dmgs.get('universal')?.size ?? 0
  if (universal <= 0) return null
  return {
    aarch64: formatDownloadSize(aarch64),
    x86_64: formatDownloadSize(x86_64),
    universal: formatDownloadSize(universal),
  }
}

/** The macOS DMG's file name for an arch (`aarch64`, `x86_64`, `universal`). */
export function macDmgFile(manifest: UpdateManifest, arch: string): string {
  const fromManifest = downloadsByArch(manifest, 'macos', 'dmg').get(arch)?.file
  return fromManifest ?? `Cmdr_${manifest.version}_${arch === 'x86_64' ? 'x64' : arch}.dmg`
}
