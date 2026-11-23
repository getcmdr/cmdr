#!/usr/bin/env node
/**
 * Couples catalog keys to screenshots from an i18n capture run.
 *
 * Reads the capture report written by `test/e2e-playwright/i18n-capture.spec.ts`
 * (`src/lib/intl/messages/screenshots/capture-report.json`, a SIBLING of `en/`:
 * surface → its recorded keys + screenshot filename) and writes `@key.screenshot`
 * into the right `messages/en/*.json` for every key that rendered on a captured
 * surface.
 *
 * Coupling policy (one screenshot per key): a key may render on several
 * surfaces; it's assigned the FIRST surface, in the report's insertion order
 * (the spec orders surfaces narrow-to-broad, so the most specific surface wins),
 * whose image SHOWS its text, judged by OCR (`placeKeys`; the text comes from
 * `screenshot-ocr.ts`, tracked in `screenshots/ocr-text.json`, and the matcher
 * is `screenshot-text-match.ts`). An image with no OCR text, and an accessible
 * name or tooltip, is trusted as before. A key no image shows keeps the surface
 * that uses it, with `NOT_SHOWN_NOTE`. Re-runnable and idempotent: a
 * second run with the same report produces no diff. It owns `screenshot` and
 * `screenshotNote` outright: a twin carrying either one for a key this run
 * doesn't couple (its surface is gone, or no representative rule reaches it
 * anymore) loses both.
 *
 * Run via `pnpm i18n:couple` (after `pnpm i18n:shots:no-couple`), or directly with
 * `node scripts/couple-screenshots.ts`. Pass `--check` to write nothing and
 * report instead, exiting by the worst finding: 0 when clean,
 * `CHECK_EXIT_WARN` for stale couplings or a representative rule every key of
 * which has its own capture, `CHECK_EXIT_ERROR` for a structural break (a rule
 * that reaches no catalog key, or a rule or catalog screenshot naming an image
 * the report doesn't have). `desktop-message-screenshots-fresh` maps those
 * codes; any other non-zero code means the script itself failed.
 *
 * The `@key` metadata (including `screenshot` and `screenshotNote`) is stripped
 * by the runtime and by `gen-message-keys.ts`, so this never changes rendered
 * output or the key union.
 *
 * Two kinds of coupling, in two passes:
 *  - DIRECT: a key that rendered on a captured surface gets that surface's
 *    screenshot (`@key.screenshot`), no note. The precise capture.
 *  - REPRESENTATIVE: for a key STILL uncoupled after the direct pass that matches
 *    a curated `REPRESENTATIVE_SCREENSHOTS` prefix, it gets a STAND-IN screenshot
 *    (a real capture of the same panel/toast/dialog where the string appears)
 *    plus a `@key.screenshotNote` explaining the mapping. This raises coverage AND
 *    shrinks the number of distinct images a translator must load. Direct always
 *    wins; a representative never overwrites a precise screenshot, and a key that
 *    later gains its own capture sheds its representative note.
 *
 * Alongside coupling, it writes a TRACKED coverage report
 * (`messages/screenshots/coverage-report.md`), rendered by
 * `screenshot-coverage-report.ts`: per catalog area, how many keys are direct,
 * representative, uncoupled, or native, plus a review of the captured surfaces.
 * Coverage is partial by design until the driver covers the full surface
 * inventory, so the report says so rather than implying gaps are bugs.
 *
 * The pure cores are exported (`couplingsFromReport`, `buildCouplings`,
 * `coupleCatalog`, `findStructuralProblems`, `checkSummary`, `fileForKey`) so they're
 * unit-testable without touching the real catalogs (see
 * `couple-screenshots.test.ts`). The CLI shell below is only file I/O around
 * those cores, and runs only when invoked as a script.
 */

import { readFileSync, writeFileSync, existsSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'
import { type RepresentativeMapping, REPRESENTATIVE_SCREENSHOTS } from './representative-screenshots.ts'
import { type OcrText, refreshOcrText } from './screenshot-ocr.ts'
import {
  type CaptureReport,
  type ShowsVerdict,
  NOT_SHOWN_NOTE,
  TRUST_EVERY_SURFACE,
  couplingsFromReport,
  ocrVerdict,
  placeKeys,
} from './screenshot-placement.ts'
import { setTwinField } from './catalog-twin-field.ts'
import {
  buildCoverageReport,
  buildSurfaceReview,
  renderCoverageReport,
  renderSurfaceReview,
} from './screenshot-coverage-report.ts'

export { type RepresentativeMapping, REPRESENTATIVE_SCREENSHOTS }
export {
  type CaptureReport,
  type CaptureSurface,
  type ShowsVerdict,
  NOT_SHOWN_NOTE,
  couplingsFromReport,
  ocrVerdict,
  placeKeys,
} from './screenshot-placement.ts'

/**
 * The catalog filename a key belongs to: first dot-segment + `.json`.
 */
export function fileForKey(key: string): string {
  const area = key.split('.')[0]
  return `${area}.json`
}

/**
 * Returns the first representative mapping whose prefix matches `key`, or null.
 * Order matters: list more specific prefixes before broader ones.
 */
export function representativeFor(
  key: string,
  mappings: RepresentativeMapping[] = REPRESENTATIVE_SCREENSHOTS,
): RepresentativeMapping | null {
  for (const m of mappings) {
    if (key.startsWith(m.prefix)) return m
  }
  return null
}

export interface Couplings {
  /** Every key → its coupling (direct, representative, or not shown). */
  byKey: Map<string, Coupling>
  /** Keys coupled to their OWN captured screenshot. */
  directKeys: Set<string>
  /** Keys coupled to a representative stand-in. */
  representativeKeys: Set<string>
  /** Keys coupled to the surface that uses them, whose image doesn't show their text (`NOT_SHOWN_NOTE`). */
  notShownKeys: Set<string>
}

/**
 * Pure: merges DIRECT capture couplings with REPRESENTATIVE stand-ins. Direct
 * couplings always win (a precise screenshot is never overwritten by a stand-in).
 * A representative coupling is added only for a key that is (a) still uncoupled
 * after the direct pass, (b) matches a representative `prefix`, and (c) whose
 * representative screenshot is one the capture run actually produced
 * (`capturedScreenshots`), never pointing a key at an image that doesn't exist.
 * @param directKeyToScreenshot key → captured screenshot.
 * @param allKeys every renderable catalog key.
 * @param capturedScreenshots filenames present in the capture report.
 * @param notShownKeyToScreenshot keys no captured image shows, each with the first surface that
 *   looked it up (`placeKeys`); coupled there with `NOT_SHOWN_NOTE` when no rule reaches them.
 */
export function buildCouplings(
  directKeyToScreenshot: Map<string, string>,
  allKeys: Iterable<string>,
  capturedScreenshots: Set<string>,
  mappings: RepresentativeMapping[] = REPRESENTATIVE_SCREENSHOTS,
  notShownKeyToScreenshot = new Map<string, string>(),
): Couplings {
  const byKey = new Map<string, Coupling>()
  const directKeys = new Set<string>()
  const representativeKeys = new Set<string>()
  const notShownKeys = new Set<string>()

  for (const [key, screenshot] of directKeyToScreenshot) {
    byKey.set(key, { screenshot })
    directKeys.add(key)
  }

  for (const key of allKeys) {
    if (directKeys.has(key)) continue // direct wins
    const rep = representativeFor(key, mappings)
    if (!rep) continue
    if (!capturedScreenshots.has(rep.screenshot)) continue // its image wasn't captured
    byKey.set(key, { screenshot: rep.screenshot, note: rep.note })
    representativeKeys.add(key)
  }

  // Last: a key no image shows and no rule stands in for keeps the surface that uses it, with
  // a note saying so, rather than passing that image off as a picture of the string.
  for (const [key, screenshot] of notShownKeyToScreenshot) {
    if (byKey.has(key)) continue
    byKey.set(key, { screenshot, note: NOT_SHOWN_NOTE })
    notShownKeys.add(key)
  }

  return { byKey, directKeys, representativeKeys, notShownKeys }
}

/**
 * Something wrong with the coupling inputs themselves, found without a capture
 * run: every input is tracked (the report, the catalogs, the rules), so a key or
 * surface rename breaks one in the same commit that caused it.
 */
export type StructuralFinding =
  /** A representative rule that is the first match for no catalog key (renamed away, or shadowed by earlier rules). */
  | { kind: 'deadRule'; prefix: string }
  /** A representative rule whose stand-in image the report doesn't have, so it silently couples nothing. */
  | { kind: 'missingRuleTarget'; prefix: string; screenshot: string }
  /** A catalog `@key.screenshot` naming an image the report doesn't have. */
  | { kind: 'unknownScreenshot'; key: string; screenshot: string }
  /** A representative rule every key of which has its own capture, so it stands in for nothing. */
  | { kind: 'redundantRule'; prefix: string; keys: number }

/** `--check` exit code when the worst finding only warns (stale couplings, a redundant rule). */
export const CHECK_EXIT_WARN = 20
/** `--check` exit code for a structural break. Both codes sit clear of Node's own, so a crash can't pass for a finding. */
export const CHECK_EXIT_ERROR = 21

/**
 * Pure: the structural findings for one set of inputs.
 * @param allKeys every catalog key.
 * @param screenshotByKey each key's current `@key.screenshot`, as written in the catalogs.
 */
export function findStructuralProblems(
  report: CaptureReport,
  allKeys: Iterable<string>,
  screenshotByKey: Map<string, string>,
  mappings: RepresentativeMapping[] = REPRESENTATIVE_SCREENSHOTS,
  shows: ShowsVerdict = TRUST_EVERY_SURFACE,
): StructuralFinding[] {
  const captured = new Set(Object.values(report).map((surface) => surface.screenshot))
  const direct = couplingsFromReport(report, shows)
  // A rule "reaches" the keys it's the FIRST match for, the same way
  // `representativeFor` picks, so a broad rule whose keys all sit under earlier
  // specific ones counts as dead.
  const reached = new Map<RepresentativeMapping, string[]>(mappings.map((rule) => [rule, []]))
  for (const key of allKeys) {
    const rule = representativeFor(key, mappings)
    if (rule) reached.get(rule)?.push(key)
  }

  const findings: StructuralFinding[] = []
  for (const rule of mappings) {
    const keys = reached.get(rule) ?? []
    if (keys.length === 0) findings.push({ kind: 'deadRule', prefix: rule.prefix })
    if (!captured.has(rule.screenshot)) {
      findings.push({ kind: 'missingRuleTarget', prefix: rule.prefix, screenshot: rule.screenshot })
    }
    if (keys.length > 0 && keys.every((key) => direct.has(key))) {
      findings.push({ kind: 'redundantRule', prefix: rule.prefix, keys: keys.length })
    }
  }
  for (const [key, screenshot] of screenshotByKey) {
    if (!captured.has(screenshot)) findings.push({ kind: 'unknownScreenshot', key, screenshot })
  }
  return findings
}

/** Whether a finding fails the check. A redundant rule only costs a translator nothing, so it warns. */
export function isBreakingFinding(finding: StructuralFinding): boolean {
  return finding.kind !== 'redundantRule'
}

/** The `--check` exit code: the worst of the structural findings and the stale couplings. */
export function checkExitCode(findings: StructuralFinding[], staleCount: number): number {
  if (findings.some((finding) => isBreakingFinding(finding))) return CHECK_EXIT_ERROR
  if (findings.length > 0 || staleCount > 0) return CHECK_EXIT_WARN
  return 0
}

/** What `--check` found, counted, for its one-line summary. */
export interface CheckCounts {
  broken: number
  redundantRules: number
  stale: number
  /** Keys the capture report names that no catalog has anymore (renamed or deleted since the capture). */
  removedKeys: number
}

/**
 * Pure: the one-line summary `--check` prints first, so the check runner can
 * show it alone and keep the bulleted detail for the saved output. Names each
 * nonzero count with its fix, joined as a prose list.
 */
export function checkSummary(counts: CheckCounts): string {
  const plural = (n: number, one: string, many: string): string => `${String(n)} ${n === 1 ? one : many}`
  const parts: string[] = []
  if (counts.broken > 0) {
    parts.push(plural(counts.broken, 'broken screenshot rule or coupling', 'broken screenshot rules or couplings'))
  }
  if (counts.stale > 0) {
    const it = counts.stale === 1 ? 'it' : 'them'
    parts.push(`${plural(counts.stale, 'stale coupling', 'stale couplings')} (\`pnpm i18n:couple\` rewrites ${it})`)
  }
  if (counts.redundantRules > 0) {
    parts.push(plural(counts.redundantRules, 'redundant representative rule', 'redundant representative rules'))
  }
  if (counts.removedKeys > 0) {
    parts.push(
      `${plural(counts.removedKeys, 'key', 'keys')} the capture report still names but the catalogs dropped ` +
        '(the next `pnpm i18n:shots` forgets them)',
    )
  }
  if (parts.length <= 1) return parts.join('')
  return `${parts.slice(0, -1).join(', ')}, and ${parts[parts.length - 1]}`
}

export interface Coupling {
  /** The screenshot filename to write to `@key.screenshot`. */
  screenshot: string
  /**
   * An optional translator note (`@key.screenshotNote`). Present for
   * REPRESENTATIVE couplings (a stand-in image), absent for DIRECT (captured)
   * ones. When absent, any existing `screenshotNote` is REMOVED, so a key that
   * gains a direct capture sheds its old representative note.
   */
  note?: string
}

/** One missing/stale coupling for `--check`: the write that WOULD happen. */
export interface StaleCoupling {
  key: string
  /** The screenshot the run would write, or undefined when it clears the key's coupling. */
  screenshot: string | undefined
  current: string | undefined
}

export interface CoupleResult {
  /** The catalog TEXT after coupling (line-surgical; all other bytes byte-identical). */
  text: string
  /** Whether any `@key.screenshot`/`@key.screenshotNote` was added, updated, or removed. */
  changed: boolean
  /** How many keys were (re)coupled. */
  couplingCount: number
  /** `key → screenshot` for keys whose twin has no description. */
  coupledWithoutDescription: string[]
  /** Keys requested but absent from this catalog. */
  missingKeys: string[]
  /** Keys present but with no `@key` twin to host the screenshot (skipped). */
  missingTwins: string[]
  /** For `--check`: keys whose coupling is missing/stale (the writes that WOULD happen). */
  stale: StaleCoupling[]
  /** How many twins lost a coupling this run no longer produces. */
  clearedCount: number
}

/**
 * Pure core: returns the catalog TEXT with `@key.screenshot` (and, for
 * representative couplings, `@key.screenshotNote`) set for every requested key,
 * edited LINE-SURGICALLY so every other byte (message values, other twin
 * fields, indentation, AND the blank lines that group the catalog) is preserved
 * exactly. (A `JSON.parse` → `JSON.stringify` round-trip would drop the
 * blank-line grouping; oxfmt doesn't restore it, so it would reflatten every
 * file on every run. The spec's gotcha: preserve oxfmt'd formatting, touch ONLY
 * the `screenshot`/`screenshotNote` fields.) Does not read or write the
 * filesystem.
 *
 * We parse the JSON once (read-only) to learn which keys exist, their current
 * screenshot/note (for idempotency), and whether the twin has a description; the
 * actual mutation is on the raw text.
 * @param rawText The catalog file contents (`messages/en/<area>.json`).
 * @param keyToCoupling key → coupling for THIS catalog.
 */
export function coupleCatalog(rawText: string, keyToCoupling: Map<string, Coupling>): CoupleResult {
  const json = JSON.parse(rawText) as Record<string, unknown>
  let text = rawText
  let couplingCount = 0
  const coupledWithoutDescription: string[] = []
  const missingKeys: string[] = []
  const missingTwins: string[] = []
  const stale: StaleCoupling[] = []

  for (const [key, coupling] of keyToCoupling) {
    const { screenshot, note } = coupling
    if (!(key in json)) {
      missingKeys.push(key)
      continue
    }
    const metaKey = `@${key}`
    const existing = json[metaKey]
    const metaIsObject = typeof existing === 'object' && existing !== null && !Array.isArray(existing)
    if (!metaIsObject) {
      // No `@key` twin to host the screenshot. The migration gave every key a
      // twin, so this is rare; skip rather than synthesize a twin in raw text.
      missingTwins.push(key)
      continue
    }
    const meta = existing as Record<string, unknown>
    const currentNote = typeof meta.screenshotNote === 'string' ? meta.screenshotNote : undefined
    // Idempotent: skip when both fields already match the desired state (note
    // absent means "no screenshotNote field").
    if (meta.screenshot === screenshot && currentNote === note) continue

    let next = setTwinField(text, metaKey, 'screenshot', screenshot)
    if (next === null) {
      // Shouldn't happen (the twin parsed as an object), but never corrupt a file.
      missingTwins.push(key)
      continue
    }
    // The note is set for representative couplings and REMOVED for direct ones,
    // so a key never carries a stale stand-in note once it has its own capture.
    const afterNote = setTwinField(next, metaKey, 'screenshotNote', note ?? null)
    if (afterNote === null) {
      missingTwins.push(key)
      continue
    }
    next = afterNote
    text = next
    const hasDescription = typeof meta.description === 'string' && meta.description.trim() !== ''
    if (!hasDescription) coupledWithoutDescription.push(`${key} → ${screenshot}`)
    couplingCount++
    stale.push({ key, screenshot, current: typeof meta.screenshot === 'string' ? meta.screenshot : undefined })
  }

  const cleared = clearUncoupledTwins(text, json, keyToCoupling)
  text = cleared.text
  stale.push(...cleared.stale)
  missingTwins.push(...cleared.unlocatable)

  // Every edit above is skipped when a field already holds its target, so any
  // byte of difference is a real change.
  const changed = text !== rawText
  const clearedCount = cleared.stale.length
  return { text, changed, couplingCount, coupledWithoutDescription, missingKeys, missingTwins, stale, clearedCount }
}

/**
 * Removes `screenshot` and `screenshotNote` from every twin whose key isn't in
 * `keyToCoupling`. The map is the whole truth for its catalog, so a coupling it
 * doesn't name is one no surface or rule produces anymore, and keeping it would
 * point a translator at the wrong image.
 * @param json The catalog as parsed BEFORE this run's edits (the twins' current fields).
 */
function clearUncoupledTwins(
  rawText: string,
  json: Record<string, unknown>,
  keyToCoupling: Map<string, Coupling>,
): { text: string; stale: StaleCoupling[]; unlocatable: string[] } {
  let text = rawText
  const stale: StaleCoupling[] = []
  const unlocatable: string[] = []
  for (const [metaKey, existing] of Object.entries(json)) {
    const key = metaKey.slice(1)
    if (!metaKey.startsWith('@') || keyToCoupling.has(key)) continue
    const meta = typeof existing === 'object' && existing !== null ? (existing as Record<string, unknown>) : {}
    const current = typeof meta.screenshot === 'string' ? meta.screenshot : undefined
    if (current === undefined && typeof meta.screenshotNote !== 'string') continue
    const withoutScreenshot = setTwinField(text, metaKey, 'screenshot', null)
    const withoutBoth =
      withoutScreenshot === null ? null : setTwinField(withoutScreenshot, metaKey, 'screenshotNote', null)
    if (withoutBoth === null) {
      unlocatable.push(key)
      continue
    }
    text = withoutBoth
    stale.push({ key, screenshot: undefined, current })
  }
  return { text, stale, unlocatable }
}

// ── CLI shell (file I/O only; skipped when imported as a module) ──────────────

/**
 * Reads every `en/*.json` catalog. Returns area → its renderable keys (the `@key`
 * metadata twins dropped, matching the runtime + codegen's key set), used to
 * compute coverage over the WHOLE catalog rather than the captured subset; and
 * key → the `@key.screenshot` its twin carries now, for the structural findings.
 * @param messagesDir absolute path to `messages/en`
 */
function readCatalogs(messagesDir: string): {
  keysByArea: Map<string, string[]>
  screenshotByKey: Map<string, string>
  english: Record<string, string>
  descriptions: Record<string, string>
} {
  const keysByArea = new Map<string, string[]>()
  const screenshotByKey = new Map<string, string>()
  const english: Record<string, string> = {}
  const descriptions: Record<string, string> = {}
  for (const name of readdirSync(messagesDir)) {
    if (!name.endsWith('.json')) continue
    const json = JSON.parse(readFileSync(join(messagesDir, name), 'utf8')) as Record<string, unknown>
    keysByArea.set(
      name.slice(0, -'.json'.length),
      Object.keys(json).filter((k) => !k.startsWith('@')),
    )
    for (const [key, value] of Object.entries(json)) {
      if (!key.startsWith('@') && typeof value === 'string') english[key] = value
    }
    for (const [metaKey, meta] of Object.entries(json)) {
      if (!metaKey.startsWith('@') || typeof meta !== 'object' || meta === null) continue
      const { screenshot, description } = meta as { screenshot?: unknown; description?: unknown }
      if (typeof screenshot === 'string') screenshotByKey.set(metaKey.slice(1), screenshot)
      if (typeof description === 'string') descriptions[metaKey.slice(1)] = description
    }
  }
  return { keysByArea, screenshotByKey, english, descriptions }
}

/** Groups key→coupling pairs by their catalog file, so each file is read/written once. */
function groupByFile(byKey: Map<string, Coupling>): Map<string, Map<string, Coupling>> {
  const byFile = new Map<string, Map<string, Coupling>>()
  for (const [key, coupling] of byKey) {
    const file = fileForKey(key)
    let m = byFile.get(file)
    if (m === undefined) {
      m = new Map()
      byFile.set(file, m)
    }
    m.set(key, coupling)
  }
  return byFile
}

interface CoupleAllResult {
  changedFiles: string[]
  couplingCount: number
  clearedCount: number
  staleForCheck: string[]
  coupledWithoutDescription: string[]
  missingTwins: string[]
  /** Report keys no catalog has anymore (or whose whole area catalog is gone). */
  removedKeys: string[]
}

/**
 * Couples every catalog file on disk, including one this run couples nothing in
 * (its stale couplings still need clearing). With `checkOnly`, collects stale
 * couplings instead of writing; otherwise writes the changed files. Collects
 * drift (a coupled area with no catalog, missing keys) for the caller to print.
 */
function coupleAllFiles(
  byFile: Map<string, Map<string, Coupling>>,
  messagesDir: string,
  checkOnly: boolean,
): CoupleAllResult {
  const changedFiles: string[] = []
  let couplingCount = 0
  let clearedCount = 0
  const staleForCheck: string[] = []
  const coupledWithoutDescription: string[] = []
  const missingTwins: string[] = []
  const removedKeys: string[] = []

  const catalogFiles = readdirSync(messagesDir).filter((name) => name.endsWith('.json'))
  for (const [file, keys] of byFile) {
    if (!catalogFiles.includes(file)) {
      for (const key of keys.keys()) removedKeys.push(`${key} (no ${file} catalog)`)
    }
  }

  for (const file of catalogFiles) {
    const filePath = join(messagesDir, file)
    const result = coupleCatalog(readFileSync(filePath, 'utf8'), byFile.get(file) ?? new Map<string, Coupling>())

    for (const key of result.missingKeys) removedKeys.push(`${key} (not in ${file})`)
    for (const key of result.missingTwins) {
      missingTwins.push(`${key} (in ${file})`)
    }
    couplingCount += result.couplingCount
    clearedCount += result.clearedCount
    coupledWithoutDescription.push(...result.coupledWithoutDescription)
    if (checkOnly) {
      for (const { key, screenshot, current } of result.stale) {
        const target = screenshot === undefined ? 'no screenshot (stale coupling to clear)' : screenshot
        staleForCheck.push(`${key} → ${target} (currently ${JSON.stringify(current)})`)
      }
      continue
    }
    if (result.changed) {
      writeFileSync(filePath, result.text)
      changedFiles.push(filePath)
    }
  }

  return {
    changedFiles,
    couplingCount,
    clearedCount,
    staleForCheck,
    coupledWithoutDescription,
    missingTwins,
    removedKeys,
  }
}

/** One finding as a line a person can act on. */
function describeFinding(finding: StructuralFinding): string {
  switch (finding.kind) {
    case 'deadRule':
      return `representative rule \`${finding.prefix}\` is the first match for no catalog key (renamed away, or every key it matches sits under an earlier rule); fix or delete it in \`representative-screenshots.ts\``
    case 'missingRuleTarget':
      return `representative rule \`${finding.prefix}\` points at ${finding.screenshot}, which \`capture-report.json\` doesn't have, so it couples nothing; aim it at a captured surface`
    case 'unknownScreenshot':
      return `${finding.key} names ${finding.screenshot}, which \`capture-report.json\` doesn't have; run \`pnpm i18n:couple\``
    case 'redundantRule':
      return `representative rule \`${finding.prefix}\` stands in for nothing: all ${String(finding.keys)} key(s) it reaches have their own capture; consider deleting it`
  }
}

/** Prints one headed, bulleted section to stderr, or nothing when it's empty. */
function printSection(header: string, lines: string[]): void {
  if (lines.length === 0) return
  console.error(`${header} (${String(lines.length)}):`)
  for (const line of lines) console.error(`  - ${line}`)
}

/** Prints the structural findings (breaking first), the stale couplings, and the removed keys. */
function printFindings(findings: StructuralFinding[], stale: string[], removedKeys: string[]): void {
  const describe = (list: StructuralFinding[]): string[] => list.map((finding) => describeFinding(finding))
  printSection(
    'Broken screenshot rules and couplings',
    describe(findings.filter((finding) => isBreakingFinding(finding))),
  )
  printSection('Redundant representative rules', describe(findings.filter((finding) => !isBreakingFinding(finding))))
  printSection('Stale screenshot couplings, which `pnpm i18n:couple` rewrites', stale)
  printSection('Keys the capture report names but the catalogs dropped, which the next capture forgets', removedKeys)
}

function main() {
  const here = dirname(fileURLToPath(import.meta.url))
  const desktopDir = join(here, '..')
  const messagesRoot = join(desktopDir, 'src', 'lib', 'intl', 'messages')
  // Catalog writes go under `messages/en/`; the capture report lives in
  // `messages/screenshots/`, a SIBLING of `en/` (where the spec writes it).
  const messagesDir = join(messagesRoot, 'en')
  const reportPath = join(messagesRoot, 'screenshots', 'capture-report.json')

  const checkOnly = process.argv.includes('--check')

  if (!existsSync(reportPath)) {
    console.error(`No capture report at ${reportPath}.\nRun \`pnpm i18n:shots\` to capture and couple in one go.`)
    process.exit(1)
  }

  const report = JSON.parse(readFileSync(reportPath, 'utf8')) as CaptureReport

  // Every renderable catalog key (for the representative pass + coverage).
  const { keysByArea, screenshotByKey, english, descriptions } = readCatalogs(messagesDir)
  const allKeys = [...keysByArea.values()].flat()
  // Screenshots the capture run actually produced (a representative may only
  // point at one of these, never at a missing image).
  const capturedScreenshots = new Set(Object.values(report).map((s) => s.screenshot))

  // Direct (captured) couplings, then representative stand-ins for the keys still
  // uncoupled. Direct always wins.
  // What each image shows. `--check` (CI, any OS) reads the tracked text as it is; a coupling
  // run first reads any image whose bytes changed, where it can (macOS with the PNGs).
  const screenshotsDir = join(messagesRoot, 'screenshots')
  const ocrPath = join(screenshotsDir, 'ocr-text.json')
  const trackedOcr = existsSync(ocrPath) ? (JSON.parse(readFileSync(ocrPath, 'utf8')) as OcrText) : {}
  const ocrText = checkOnly
    ? trackedOcr
    : refreshOcrText(screenshotsDir, [...capturedScreenshots], trackedOcr, (line) => {
        console.log(`[ocr] ${line}`)
      })
  const shows = ocrVerdict(ocrText, english, descriptions)
  const placed = placeKeys(report, shows)
  const { byKey, directKeys, representativeKeys, notShownKeys } = buildCouplings(
    placed.shown,
    allKeys,
    capturedScreenshots,
    REPRESENTATIVE_SCREENSHOTS,
    placed.notShown,
  )

  // Group target keys by their catalog file so each file is read/written once.
  const byFile = groupByFile(byKey)
  const {
    changedFiles,
    couplingCount,
    clearedCount,
    staleForCheck,
    coupledWithoutDescription,
    missingTwins,
    removedKeys,
  } = coupleAllFiles(byFile, messagesDir, checkOnly)

  if (checkOnly) {
    // Read against the catalogs as committed: a screenshot the report lacks is a
    // break for as long as a twin still names it.
    const findings = findStructuralProblems(report, allKeys, screenshotByKey, REPRESENTATIVE_SCREENSHOTS, shows)
    // Removed keys ride along in the summary but never raise the exit code on
    // their own: only a fresh capture clears them, and that's an E2E run.
    const exitCode = checkExitCode(findings, staleForCheck.length)
    if (exitCode === 0) {
      console.log('Every screenshot coupling matches the capture report, and every representative rule is live.')
      process.exit(0)
    }
    // The summary goes first and alone on its line: the check runner prints it
    // and saves the sections below to its full-output log.
    const broken = findings.filter((finding) => isBreakingFinding(finding)).length
    console.error(
      checkSummary({
        broken,
        redundantRules: findings.length - broken,
        stale: staleForCheck.length,
        removedKeys: removedKeys.length,
      }),
    )
    printFindings(findings, staleForCheck, removedKeys)
    process.exit(exitCode)
  }

  // This run just rewrote every twin from the report, so only the rules can still
  // be broken; a finding here doesn't stop the write, it tells the person running it.
  printFindings(findStructuralProblems(report, allKeys, new Map(), REPRESENTATIVE_SCREENSHOTS, shows), [], removedKeys)
  writeFileSync(ocrPath, JSON.stringify(ocrText, null, 2) + '\n')

  console.log(
    `Coupled ${String(couplingCount)} key(s) to screenshots and cleared ${String(clearedCount)} stale coupling(s) ` +
      `across ${String(changedFiles.length)} catalog file(s) ` +
      `(${String(directKeys.size)} direct, ${String(representativeKeys.size)} representative, ` +
      `${String(notShownKeys.size)} on a surface whose image doesn't show them).`,
  )

  if (coupledWithoutDescription.length > 0) {
    console.warn(
      `Coupled ${String(coupledWithoutDescription.length)} key(s) that lack a description twin (screenshot-only; checks still pass, but the catalog convention wants a description):`,
    )
    for (const line of coupledWithoutDescription) console.warn(`  - ${line}`)
  }

  if (missingTwins.length > 0) {
    console.warn(
      `Skipped ${String(missingTwins.length)} key(s) with no @key twin to host the screenshot (author a twin to couple them):`,
    )
    for (const line of missingTwins) console.warn(`  - ${line}`)
  }

  // Coverage report (Decision 4: no silent gaps): a tracked, text artifact that
  // shows which areas have screenshots (direct vs representative) and which keys
  // remain uncoupled.
  const coverage = buildCoverageReport(directKeys, representativeKeys, keysByArea, notShownKeys)
  const coveragePath = join(messagesRoot, 'screenshots', 'coverage-report.md')
  // The per-key table, then the per-SURFACE review (redundant surfaces, reduced
  // zoom). One tracked artifact, so a reviewer reads both in the same diff.
  writeFileSync(coveragePath, renderCoverageReport(coverage) + '\n' + renderSurfaceReview(buildSurfaceReview(report)))
  console.log(
    `Wrote coverage report: ${String(coverage.direct)} direct + ${String(coverage.representative)} representative ` +
      `/ ${String(coverage.total)} keys → ${coveragePath}`,
  )

  // Safety net: confirm every file this run wrote is oxfmt-clean. For the
  // surgical catalog edits it's a no-op in practice (we preserve oxfmt's shape),
  // but the coverage report is rendered from scratch and its reflowed prose and
  // padded tables are oxfmt's call, not ours; without this the next `pnpm check`
  // fails on a file nobody hand-edited.
  const toFormat = [...changedFiles, coveragePath, ocrPath]
  const res = spawnSync('pnpm', ['exec', 'oxfmt', ...toFormat], { cwd: desktopDir, stdio: 'inherit' })
  if (res.status !== 0) {
    console.warn('oxfmt did not exit cleanly; run `pnpm exec oxfmt` on the changed files manually.')
  }
}

// Run the CLI only when executed directly, not when imported by a test.
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main()
}
