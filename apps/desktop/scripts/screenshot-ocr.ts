/**
 * The text in each translator screenshot, read once per image and kept in a TRACKED file
 * (`messages/screenshots/ocr-text.json`), so the coupler's OCR check runs anywhere.
 *
 * Reading text needs macOS Vision (`screenshot-ocr.swift`) and the PNGs, which only a Mac
 * with a capture has. Matching it against the English is pure and cheap. So the expensive,
 * Mac-only half writes its answer into the repo keyed by each image's SHA-256, and
 * everything after it (the coupler, its `--check` in CI on Linux) reads that file:
 *
 *  - An entry whose hash matches the image on disk is reused as it is: the cache.
 *  - An image whose hash moved (a recapture) is read again.
 *  - An image that isn't on disk (CI, a checkout without the PNGs) keeps its entry.
 *  - Off macOS, or with no `swiftc`, nothing is read and every entry stays: a clean skip.
 *  - An entry for a screenshot the report no longer names is dropped.
 *
 * The Vision helper is compiled on first use per version of its source (~10 s), into the
 * capture cache beside the countdown's estimates.
 */

import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/** One screenshot's text, and the image it was read from. */
export interface OcrEntry {
  sha256: string
  lines: string[]
}

/** Screenshot filename → its text. Sorted by filename when written, so a rerun is byte-stable. */
export type OcrText = Record<string, OcrEntry>

const here = dirname(fileURLToPath(import.meta.url))
const OCR_SOURCE = join(here, 'screenshot-ocr.swift')
const OCR_CACHE = join(homedir(), 'Library', 'Caches', 'com.veszelovszki.cmdr-capture', 'ocr')

/** The images to read: those whose hash moved, or that have no entry. Pure. */
export function imagesToRead(
  wanted: Iterable<string>,
  existing: OcrText,
  hashOnDisk: (file: string) => string | null,
): { file: string; sha256: string }[] {
  const out: { file: string; sha256: string }[] = []
  for (const file of wanted) {
    const sha256 = hashOnDisk(file)
    if (sha256 === null || (existing[file] as OcrEntry | undefined)?.sha256 === sha256) continue
    out.push({ file, sha256 })
  }
  return out
}

/** The next tracked file: kept entries plus fresh reads, only for `wanted`, sorted. Pure. */
export function mergeOcrText(wanted: Iterable<string>, existing: OcrText, fresh: OcrText): OcrText {
  const merged: OcrText = {}
  for (const file of [...new Set(wanted)].sort()) {
    if (Object.hasOwn(fresh, file)) merged[file] = fresh[file]
    else if (Object.hasOwn(existing, file)) merged[file] = existing[file]
  }
  return merged
}

function sha256Of(path: string): string | null {
  if (!existsSync(path)) return null
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

/** The compiled Vision helper for the current source, or null where it can't run. */
function ensureOcrBinary(log: (line: string) => void): string | null {
  if (process.platform !== 'darwin') {
    log('not on macOS, so no screenshot is read; the OCR check uses the tracked text as it is')
    return null
  }
  const hash = createHash('sha256').update(readFileSync(OCR_SOURCE)).digest('hex').slice(0, 16)
  const binary = join(OCR_CACHE, `cmdr-screenshot-ocr-${hash}`)
  if (existsSync(binary)) return binary
  mkdirSync(OCR_CACHE, { recursive: true })
  log('compiling the OCR helper (once per version of its source)…')
  const building = `${binary}.building`
  const res = spawnSync('swiftc', ['-O', '-parse-as-library', '-o', building, OCR_SOURCE], { encoding: 'utf8' })
  if (res.error !== undefined || res.status !== 0) {
    rmSync(building, { force: true })
    log(`couldn't compile the OCR helper, so no screenshot is read: ${res.stderr.trim() || String(res.error)}`)
    return null
  }
  renameSync(building, binary)
  return binary
}

/** One line of the helper's output. */
interface OcrLine {
  file: string
  lines?: string[]
  error?: string
}

/**
 * Brings the tracked text up to date with the images in `screenshotsDir`, reading only the
 * ones that changed. Returns the new file contents; the caller writes it.
 * @param wanted every screenshot the capture report names
 */
export function refreshOcrText(
  screenshotsDir: string,
  wanted: readonly string[],
  existing: OcrText,
  log: (line: string) => void,
): OcrText {
  const toRead = imagesToRead(wanted, existing, (file) => sha256Of(join(screenshotsDir, file)))
  const fresh: OcrText = {}
  if (toRead.length > 0) {
    const binary = ensureOcrBinary(log)
    if (binary !== null) {
      log(`reading the text in ${String(toRead.length)} screenshot(s)…`)
      const res = spawnSync(
        binary,
        toRead.map(({ file }) => join(screenshotsDir, file)),
        { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
      )
      if (res.status !== 0) log(`the OCR helper exited ${String(res.status)}: ${res.stderr.trim()}`)
      const byPath = new Map(toRead.map(({ file, sha256 }) => [join(screenshotsDir, file), { file, sha256 }]))
      for (const raw of res.stdout.split('\n')) {
        if (raw.trim() === '') continue
        const line = JSON.parse(raw) as OcrLine
        const target = byPath.get(line.file)
        if (target === undefined) continue
        if (line.lines === undefined) {
          log(`couldn't read ${target.file}: ${line.error ?? 'no text came back'}`)
          continue
        }
        fresh[target.file] = { sha256: target.sha256, lines: line.lines }
      }
    }
  }
  return mergeOcrText(wanted, existing, fresh)
}
