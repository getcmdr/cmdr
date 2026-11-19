/**
 * What the site and the updater need from an update manifest, for `fetch-live-manifest.ts`, which refuses a live one
 * that lacks it (the Docker build fails and the deploy keeps the old site). Its own module so the unit tests
 * (`e2e/download-platforms.spec.ts`) can run it without the script's fetch.
 */

const platformKeys = ['darwin-aarch64', 'darwin-x86_64', 'darwin-universal']

function isDownload(d: unknown): boolean {
  const e = d as Record<string, unknown> | null
  return (
    typeof e?.os === 'string' &&
    typeof e.arch === 'string' &&
    typeof e.kind === 'string' &&
    typeof e.file === 'string' &&
    typeof e.size === 'number'
  )
}

/**
 * The download sizes and names: `downloads` (from the release asset manifest) or, in manifests from before it,
 * `dmgSizes`. `src/lib/release-downloads.ts` reads either. A `downloads` list must carry the universal DMG: it's
 * every button's link with JS off and for any visitor whose Mac arch is unknown, so without it the site would have
 * buttons with no link (or an Apple silicon DMG for an Intel Mac).
 */
function problemWithDownloads(m: Record<string, unknown>): string | null {
  if (m.downloads !== undefined) {
    if (!Array.isArray(m.downloads) || !m.downloads.every(isDownload)) return 'a malformed `downloads` list'
    const list = m.downloads as { os: string; arch: string; kind: string }[]
    const universal = list.some((d) => d.os === 'macos' && d.kind === 'dmg' && d.arch === 'universal')
    return universal ? null : 'a `downloads` list without the universal macOS DMG'
  }
  const sizes = m.dmgSizes as Record<string, unknown> | undefined
  return typeof sizes?.universal === 'number' ? null : 'no `downloads` or `dmgSizes`'
}

/** The fields the site and the updater read; the reason when one is missing or malformed. */
export function liveManifestProblem(manifest: unknown): string | null {
  if (typeof manifest !== 'object' || manifest === null) return 'not a JSON object'
  const m = manifest as Record<string, unknown>
  if (typeof m.version !== 'string' || !/^\d+\.\d+\.\d+$/.test(m.version)) return 'no X.Y.Z `version`'
  if (typeof m.notes !== 'string' || typeof m.pub_date !== 'string') return 'no `notes` or `pub_date`'
  const downloadsProblem = problemWithDownloads(m)
  if (downloadsProblem) return downloadsProblem
  const platforms = m.platforms as Record<string, Record<string, unknown> | undefined> | undefined
  for (const key of platformKeys) {
    const platform = platforms?.[key]
    if (typeof platform?.url !== 'string' || typeof platform.signature !== 'string') return `no \`${key}\` platform`
  }
  return null
}
