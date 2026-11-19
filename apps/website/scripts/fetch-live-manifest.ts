/**
 * Writes the live update manifest over `public/latest.json` before a production build.
 *
 * The live manifest is the `latest.json` asset of the release GitHub marks "Latest". The release
 * pipeline's `publish` job uploads it and only then flips that mark, so this always reads a complete
 * manifest, and a rollback is a flip back. The committed `public/latest.json` is a frozen copy that
 * dev and the check lanes build with; only the Docker build (`../Dockerfile`) runs this, so
 * getcmdr.com, which every installed Cmdr polls, serves the live one. `docs/guides/releasing.md`
 * § What a release publishes.
 *
 * Any failure exits non-zero, which fails the Docker build, so the deploy keeps the old site up
 * rather than serving a stale or broken manifest.
 */
import { writeFileSync } from 'node:fs'
import { liveManifestProblem } from './live-manifest-check.ts'

const manifestUrl = 'https://github.com/getcmdr/cmdr/releases/latest/download/latest.json'
const target = new URL('../public/latest.json', import.meta.url)

async function fetchManifest(): Promise<string> {
  let lastError: unknown
  for (let attempt = 1; attempt <= 4; attempt++) {
    try {
      const response = await fetch(manifestUrl, { redirect: 'follow' })
      if (response.ok) return await response.text()
      lastError = new Error(`HTTP ${String(response.status)}`)
    } catch (error) {
      lastError = error
    }
    await new Promise((resolve) => setTimeout(resolve, attempt * 2000))
  }
  throw new Error(`Couldn't fetch ${manifestUrl}: ${String(lastError)}`)
}

const body = await fetchManifest()
const manifest: unknown = JSON.parse(body)
const problem = liveManifestProblem(manifest)
if (problem) {
  console.error(`The live manifest at ${manifestUrl} has ${problem}, so the build stops here.`)
  process.exit(1)
}
writeFileSync(target, body)
console.log(`Wrote the live manifest (${(manifest as { version: string }).version}) to public/latest.json.`)
