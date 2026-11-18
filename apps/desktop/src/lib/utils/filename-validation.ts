/** Client-side filename validation for instant keystroke feedback. */

import { tString } from '#lib/intl/messages.svelte.js'
import { fromUserInput, isLocalPath, joinPath, parsePath } from '#lib/path/grammar.js'
import { fileExtension, kindOfRenamedName, type NameKind } from './file-extension'
import type { NameRules, ValidationError } from '#lib/ipc/bindings.js'

/** Defaults match macOS. Call initPathLimits() at startup to get platform-specific values from the backend. */
let maxNameBytes = 255
let maxPathBytes = 1024

/** Fetches platform-specific limits from the backend. Call once at app startup. */
export async function initPathLimits(): Promise<void> {
  const { getPathLimits } = await import('#lib/tauri-commands/index.js')
  const limits = await getPathLimits()
  maxNameBytes = limits.maxNameBytes
  maxPathBytes = limits.maxPathBytes
}

export type ValidationSeverity = 'error' | 'warning' | 'ok'

export interface ValidationResult {
  severity: ValidationSeverity
  message: string
}

const OK_RESULT: ValidationResult = { severity: 'ok', message: '' }

/** ICU `select` discriminator for the file-vs-folder message variants. */
function nameKind(isDir: boolean): 'file' | 'folder' {
  return isDir ? 'folder' : 'file'
}

/** The device names Windows reserves, matched ignoring case against the part before the first period. */
const WINDOWS_RESERVED_NAMES = new Set([
  'CON', 'PRN', 'AUX', 'NUL',
  'COM0', 'COM1', 'COM2', 'COM3', 'COM4', 'COM5', 'COM6', 'COM7', 'COM8', 'COM9', 'COM¹', 'COM²', 'COM³',
  'LPT0', 'LPT1', 'LPT2', 'LPT3', 'LPT4', 'LPT5', 'LPT6', 'LPT7', 'LPT8', 'LPT9', 'LPT¹', 'LPT²', 'LPT³',
]) // prettier-ignore

/** The printable characters a FAT32, exFAT, or NTFS name can't hold. */
const WINDOWS_FORBIDDEN_CHARACTERS = new Set(['"', '*', ':', '<', '>', '?', '\\', '|'])

/** How a refused character reads in the message: itself, `NUL`, or its `U+` code. */
function disallowedCharacter(ch: string, rules: NameRules): string | null {
  if (ch === '/') return '/'
  if (ch === '\0') return 'NUL'
  if (rules !== 'windows') return null
  const code = ch.codePointAt(0) ?? 0
  if (code >= 0x01 && code <= 0x1f) return `U+${code.toString(16).toUpperCase().padStart(4, '0')}`
  return WINDOWS_FORBIDDEN_CHARACTERS.has(ch) ? ch : null
}

/** The part of `name` that makes it a Windows device name, as typed, or `null`. */
function reservedDeviceName(name: string): string | null {
  const stem = (name.split('.')[0] ?? name).replace(/ +$/, '')
  // ASCII-only folding, as the Rust side's `eq_ignore_ascii_case`.
  const folded = stem.replace(/[a-z]/g, (c) => c.toUpperCase())
  return WINDOWS_RESERVED_NAMES.has(folded) ? stem : null
}

/**
 * Why a destination following `rules` refuses `name`, or `null` when it takes it: the
 * keystroke mirror of `cmdr_fs::validation::validate_filename`. Both run the cases in
 * `crates/cmdr-fs/src/name_rules_cases.json`, so the two can't drift. `rules` is the
 * folder's (`getNameRules`), ❌ never a guess from the host OS.
 */
export function nameRefusal(name: string, rules: NameRules): ValidationError | null {
  if (name.trim().length === 0) return { kind: 'empty' }
  for (const ch of name) {
    const character = disallowedCharacter(ch, rules)
    if (character !== null) return { kind: 'disallowedCharacter', character }
  }
  const bytes = new TextEncoder().encode(name).length
  if (bytes > maxNameBytes) return { kind: 'nameTooLong', bytes, max: maxNameBytes }
  if (rules !== 'posix') {
    const reserved = reservedDeviceName(name)
    if (reserved !== null) return { kind: 'reservedName', name: reserved }
  }
  if (rules === 'windows' && /[ .]$/.test(name)) return { kind: 'endsWithSpaceOrPeriod' }
  return null
}

/** The words for a name refusal, the live check's and the backend's alike, so the two read the same. */
export function validationMessage(error: ValidationError, isDir: boolean): string {
  const kind = nameKind(isDir)
  switch (error.kind) {
    case 'empty':
      return tString('fileOperations.validation.empty', { kind })
    case 'disallowedCharacter':
      return tString('fileOperations.validation.disallowedCharacter', { kind, character: error.character })
    case 'nameTooLong':
      return tString('fileOperations.validation.nameTooLong', {
        kind,
        byteCount: String(error.bytes),
        maxBytes: String(error.max),
        max: error.max,
      })
    case 'pathTooLong':
      return tString('fileOperations.validation.pathTooLong', {
        byteCount: String(error.bytes),
        maxBytes: String(error.max),
        max: error.max,
      })
    case 'reservedName':
      return tString('fileOperations.validation.reservedName', { kind, name: error.name })
    case 'endsWithSpaceOrPeriod':
      return tString('fileOperations.validation.endsWithSpaceOrPeriod', { kind })
  }
}

/** Validates a (trimmed) file or folder name for a destination following `rules`. */
export function validateName(name: string, isDir: boolean, rules: NameRules): ValidationResult {
  const refusal = nameRefusal(name, rules)
  return refusal === null ? OK_RESULT : { severity: 'error', message: validationMessage(refusal, isDir) }
}

/** Validates that a file or dir name is not empty after trimming. */
export function validateNotEmpty(name: string, isDir = false): ValidationResult {
  if (name.trim().length === 0) {
    return { severity: 'error', message: tString('fileOperations.validation.empty', { kind: nameKind(isDir) }) }
  }
  return OK_RESULT
}

/** Validates full path byte length (max 1024 bytes). */
export function validatePathLength(parentPath: string, name: string): ValidationResult {
  const fullPath = joinPath(parentPath, name.trim())
  const byteLength = new TextEncoder().encode(fullPath).length
  if (byteLength >= maxPathBytes) {
    return {
      severity: 'error',
      message: tString('fileOperations.validation.pathTooLong', {
        byteCount: String(byteLength),
        maxBytes: String(maxPathBytes),
        max: maxPathBytes,
      }),
    }
  }
  return OK_RESULT
}

/**
 * Extensions treated as equivalent: switching between any two in a group skips the rename warning.
 * Each group lists spellings of the same underlying format, lowercase, no leading dot.
 *
 * The `md`/`markdown`/`txt` group is included by deliberate choice: Markdown is plain text and
 * the routing tradeoff (different "Open with" defaults) is accepted as a feature, not a bug.
 */
const EQUIVALENT_EXTENSION_GROUPS: readonly (readonly string[])[] = [
  ['jpg', 'jpeg', 'jpe', 'jfif'],
  ['tif', 'tiff'],
  ['htm', 'html'],
  ['yml', 'yaml'],
  ['mpg', 'mpeg'],
  ['mid', 'midi'],
  ['aif', 'aiff'],
  ['qt', 'mov'],
  ['md', 'markdown', 'txt'],
]

/** Precomputed `ext -> groupIndex` map for O(1) equivalence lookup. */
const EXTENSION_TO_GROUP: ReadonlyMap<string, number> = new Map(
  EQUIVALENT_EXTENSION_GROUPS.flatMap((group, i) => group.map((ext) => [ext, i] as const)),
)

function normalizedExt(filename: string, kind: NameKind): string {
  return fileExtension(filename, kind).toLowerCase()
}

/**
 * True if the extension change is meaningful enough to warrant a confirmation.
 * Returns false for case-only changes (`photo.JPG` → `photo.jpg`) and for changes
 * between known equivalents (`photo.jpeg` → `photo.jpg`, `notes.md` → `notes.txt`),
 * so users aren't pestered to confirm what's effectively a metadata tweak.
 */
export function extensionsDifferMeaningfully(oldName: string, newName: string, kind: NameKind = 'file'): boolean {
  const trimmed = newName.trim()
  const oldExt = normalizedExt(oldName, kind)
  const newExt = normalizedExt(trimmed, kindOfRenamedName(kind, trimmed))
  if (oldExt === newExt) return false
  const oldGroup = EXTENSION_TO_GROUP.get(oldExt)
  return oldGroup === undefined || oldGroup !== EXTENSION_TO_GROUP.get(newExt)
}

/**
 * True if renaming `oldName` to `newName` changes the extension enough to confirm.
 * What an extension is comes from `file-extension.ts`, and `kind` is the
 * entry's (`nameKindOf`): a folder has one only when the backend says it's a
 * macOS package, so `release.v1.2` → `release.v1.3` asks nothing while
 * `Cmdr.app` → `Cmdr.old` does, and so does a package the backend learned of
 * from macOS (`Sketch.myBundle`). The new name's kind is `kindOfRenamedName`'s,
 * so a folder turning into a listed package (`Plugins` → `Plugins.bundle`)
 * asks too.
 */
export function extensionChangeNeedsConfirmation(oldName: string, newName: string, kind: NameKind): boolean {
  return extensionsDifferMeaningfully(oldName, newName, kind)
}

/** Validates extension change against the user's preference. */
export function validateExtensionChange(
  oldName: string,
  newName: string,
  allowExtensionChanges: 'yes' | 'no' | 'ask',
  kind: NameKind = 'file',
): ValidationResult {
  if (allowExtensionChanges === 'yes') return OK_RESULT

  if (!extensionChangeNeedsConfirmation(oldName, newName, kind)) return OK_RESULT

  if (allowExtensionChanges === 'no') {
    const ext = fileExtension(oldName, kind)
    const oldExt = ext ? `.${ext}` : ''
    return {
      severity: 'error',
      message: tString('fileOperations.validation.extensionChangeBlocked', { oldExt }),
    }
  }
  // 'ask': no error, the dialog will handle it on save
  return OK_RESULT
}

/**
 * Checks if the new name matches an existing sibling, case-insensitively on every
 * volume: on a case-insensitive one (APFS, NTFS, SMB) that's a real clash, and on a
 * case-sensitive one a case-only twin is still worth a warning (it's not a refusal).
 * So this deliberately ignores the volume's `caseSensitive`.
 * Exception: case-only rename of the same file produces no warning.
 */
export function validateConflict(newName: string, siblingNames: string[], originalName: string): ValidationResult {
  const trimmed = newName.trim()
  const trimmedLower = trimmed.toLowerCase()
  const originalLower = originalName.toLowerCase()

  for (const sibling of siblingNames) {
    if (sibling.toLowerCase() === trimmedLower) {
      // Case-only rename of the same file: no warning
      if (sibling.toLowerCase() === originalLower) continue
      return { severity: 'warning', message: tString('fileOperations.validation.conflict', { name: trimmed }) }
    }
  }
  return OK_RESULT
}

/**
 * Refuses a server with no share (`\\server`, `//server/`): the grammar reads no folder
 * there on Windows, and without this the field shows nothing until the backend refuses.
 * On macOS and Linux a `//` path is an ordinary absolute one, so this always passes there.
 */
export function validateShareNamed(path: string): ValidationResult {
  const wire = fromUserInput(path.trim())
  if (wire.startsWith('//') && parsePath(wire) === null) {
    return { severity: 'error', message: tString('fileOperations.validation.pathShareMissing') }
  }
  return OK_RESULT
}

/** Validates a full directory path (not a filename). Checks structure only, not existence. */
export function validateDirectoryPath(path: string): ValidationResult {
  const trimmed = path.trim()

  if (trimmed.length === 0) {
    return { severity: 'error', message: tString('fileOperations.validation.pathEmpty') }
  }

  // Accept absolute paths (`/…`, which is also how a volume-relative destination
  // reads on every volume; on Windows also `C:/…` or `C:\…` and a UNC share) and
  // the home shortcut (`~` or `~/…`). `~` is the app's internal stand-in for the
  // user's home dir, expanded to an absolute path by the backend. A bare `~foo` (no
  // slash) isn't a home shortcut, so it stays rejected as relative.
  // Every structural check reads the wire form, where `/` alone separates: on Windows a
  // typed `\` is a separator too (`~\Documents`, `\Users`), elsewhere it's a name character.
  const wire = fromUserInput(trimmed)
  const shareNamed = validateShareNamed(trimmed)
  if (shareNamed.severity === 'error') return shareNamed
  const isAbsolute = wire.startsWith('/') || isLocalPath(wire)
  if (!isAbsolute && wire !== '~' && !wire.startsWith('~/')) {
    return { severity: 'error', message: tString('fileOperations.validation.pathNotAbsolute') }
  }

  if (trimmed.includes('\0')) {
    return { severity: 'error', message: tString('fileOperations.validation.pathNullChar') }
  }

  const encoder = new TextEncoder()
  const totalBytes = encoder.encode(trimmed).length
  if (totalBytes >= maxPathBytes) {
    return {
      severity: 'error',
      message: tString('fileOperations.validation.pathTotalTooLong', {
        byteCount: String(totalBytes),
        maxBytes: String(maxPathBytes),
        max: maxPathBytes,
      }),
    }
  }

  const segments = wire.split('/').filter((s) => s.length > 0)
  for (const segment of segments) {
    const segmentBytes = encoder.encode(segment).length
    if (segmentBytes > maxNameBytes) {
      return {
        severity: 'error',
        message: tString('fileOperations.validation.pathSegmentTooLong', {
          byteCount: String(segmentBytes),
          maxBytes: String(maxNameBytes),
          max: maxNameBytes,
        }),
      }
    }
  }

  return OK_RESULT
}

/**
 * Runs all validation checks and returns the first error, then the first warning, or ok.
 * `rules` is the folder's filesystem's (`getNameRules`).
 */
export function validateFilename(
  newName: string,
  originalName: string,
  parentPath: string,
  siblingNames: string[],
  allowExtensionChanges: 'yes' | 'no' | 'ask',
  rules: NameRules,
  kind: NameKind = 'file',
): ValidationResult {
  const isDir = kind !== 'file'

  // Error checks
  const nameCheck = validateName(newName.trim(), isDir, rules)
  if (nameCheck.severity === 'error') return nameCheck

  const pathLen = validatePathLength(parentPath, newName)
  if (pathLen.severity === 'error') return pathLen

  const extCheck = validateExtensionChange(originalName, newName, allowExtensionChanges, kind)
  if (extCheck.severity === 'error') return extCheck

  // Warning checks
  const conflict = validateConflict(newName, siblingNames, originalName)
  if (conflict.severity === 'warning') return conflict

  return OK_RESULT
}
