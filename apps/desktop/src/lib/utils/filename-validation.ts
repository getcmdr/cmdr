/** Client-side filename validation for instant keystroke feedback. */

import { tString } from '#lib/intl/messages.svelte.js'
import { formatInteger } from '#lib/intl/number-format.js'
import { fromUserInput, isLocalPath, joinPath, parsePath } from '#lib/path/grammar.js'
import { fileExtension, kindOfRenamedName, type NameKind } from './file-extension'
import type {
  LengthLimit,
  LengthUnit,
  NameFormat,
  NameRules,
  PathLimits,
  PortabilityWarning,
  ValidationError,
} from '#lib/ipc/bindings.js'

/**
 * The host's own limits, for a path typed before any destination answers. Defaults match
 * macOS; `initPathLimits()` asks the backend at startup. A folder's own limits come with its
 * rules (`getNameRules`).
 */
let hostLimits: PathLimits = { name: { max: 255, unit: 'utf16Units' }, path: { max: 1023, unit: 'bytes' } }

/** Fetches this OS's limits from the backend. Call once at app startup. */
export async function initPathLimits(): Promise<void> {
  const { getPathLimits } = await import('#lib/tauri-commands/index.js')
  hostLimits = await getPathLimits()
}

/** How long `text` is in `unit`: UTF-8 bytes, or UTF-16 code units (JS's own `length`). */
export function lengthIn(text: string, unit: LengthUnit): number {
  return unit === 'bytes' ? new TextEncoder().encode(text).length : text.length
}

/** `text`'s length when it's past `limit`, or `null` when it fits. */
function exceededBy(text: string, limit: LengthLimit): number | null {
  const length = lengthIn(text, limit.unit)
  return length > limit.max ? length : null
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

/** How a character Windows can't use reads: itself, or its `U+` code when it's a control character. */
function windowsCharacter(ch: string): string | null {
  const code = ch.codePointAt(0) ?? 0
  if (code >= 0x01 && code <= 0x1f) return `U+${code.toString(16).toUpperCase().padStart(4, '0')}`
  return WINDOWS_FORBIDDEN_CHARACTERS.has(ch) ? ch : null
}

/**
 * The code points (inclusive ranges) besides Unicode `White_Space` that render as nothing: the
 * invisible bidi, zero-width, and format characters, and the other blank-rendering ones.
 */
const BLANK_CODE_POINTS: readonly (readonly [number, number])[] = [
  [0x202a, 0x202e],
  [0x2066, 0x2069],
  [0x200e, 0x200f],
  [0x061c, 0x061c],
  [0x200b, 0x200d],
  [0x2060, 0x2060],
  [0xfeff, 0xfeff],
  [0x00ad, 0x00ad],
  [0x034f, 0x034f],
  [0x115f, 0x1160],
  [0x3164, 0x3164],
  [0xffa0, 0xffa0],
  [0x17b4, 0x17b5],
  [0x180b, 0x180f],
  [0x2061, 0x2064],
  [0x206a, 0x206f],
  [0xfe00, 0xfe0f],
  [0x1bca0, 0x1bca3],
  [0x1d173, 0x1d17a],
  [0xe0000, 0xe0fff],
  [0x2800, 0x2800],
]

/**
 * Whether `ch` shows nothing on its own: the mirror of `cmdr_fs::validation::renders_blank`
 * (Unicode `White_Space` plus `BLANK_CODE_POINTS`). The shared case file pins the two lists together.
 */
function rendersBlank(ch: string): boolean {
  if (/^\p{White_Space}$/u.test(ch)) return true
  const c = ch.codePointAt(0) ?? 0
  return BLANK_CODE_POINTS.some(([lo, hi]) => c >= lo && c <= hi)
}

/** The part of `name` that makes it a Windows device name, as typed, or `null`. */
function reservedDeviceName(name: string): string | null {
  const stem = (name.split('.')[0] ?? name).replace(/ +$/, '')
  // ASCII-only folding, as the Rust side's `eq_ignore_ascii_case`.
  const folded = stem.replace(/[a-z]/g, (c) => c.toUpperCase())
  return WINDOWS_RESERVED_NAMES.has(folded) ? stem : null
}

/**
 * Whether `name` renders as nothing: only whitespace and invisible characters, by the Rust side's rule
 * (`renders_blank`). ❌ Not `String.prototype.trim`: it keeps NEL (U+0085), which Rust counts as blank.
 */
function isBlank(name: string): boolean {
  // Code point by code point, as Rust's `chars()`; a grapheme split would hide a blank one.
  for (const ch of name) {
    if (!rendersBlank(ch)) return false
  }
  return true
}

/** The first character in `name` (code point by code point) that Windows can't use, as it reads. */
function firstWindowsCharacter(name: string): string | null {
  for (const ch of name) {
    const shown = windowsCharacter(ch)
    if (shown !== null) return shown
  }
  return null
}

/** What every destination refuses: a blank name, `/` or NUL, `.` or `..`, or one past the folder's length. */
function universalRefusal(name: string, rules: NameRules): ValidationError | null {
  if (isBlank(name)) return { kind: 'empty' }
  for (const ch of name) {
    if (ch === '/') return { kind: 'disallowedCharacter', character: '/' }
    if (ch === '\0') return { kind: 'disallowedCharacter', character: 'NUL' }
  }
  if (name === '.' || name === '..') return { kind: 'dotName' }
  const length = exceededBy(name, rules.nameLimit)
  if (length !== null) return { kind: 'nameTooLong', length, max: rules.nameLimit.max, unit: rules.nameLimit.unit }
  return null
}

/** One class of Windows trouble in a name, with the tier the destination gives it. */
interface PortabilityClass {
  tier: NameRules['windowsCharacters']
  error: ValidationError
  warn: PortabilityWarning
}

/** The classes of Windows trouble `name` has, in the order the Rust side names them. */
function portabilityClasses(name: string, rules: NameRules): PortabilityClass[] {
  const classes: PortabilityClass[] = []
  const character = firstWindowsCharacter(name)
  if (character !== null) {
    classes.push({
      tier: rules.windowsCharacters,
      error: { kind: 'disallowedCharacter', character },
      warn: { kind: 'windowsCharacter', character },
    })
  }
  const reserved = reservedDeviceName(name)
  if (reserved !== null) {
    classes.push({
      tier: rules.reservedNames,
      error: { kind: 'reservedName', name: reserved },
      warn: { kind: 'reservedName', name: reserved },
    })
  }
  if (/[ .]$/.test(name)) {
    classes.push({
      tier: rules.windowsCharacters,
      error: { kind: 'endsWithSpaceOrPeriod' },
      warn: { kind: 'endsWithSpaceOrPeriod' },
    })
  }
  return classes
}

/** What a destination does with a name: refuses it, takes it with a portability warning, or takes it. */
export type NameVerdict =
  | { kind: 'ok' }
  | { kind: 'refused'; error: ValidationError }
  | { kind: 'warned'; warning: PortabilityWarning }

/**
 * What a destination following `rules` does with `name`: the keystroke mirror of
 * `cmdr_fs::validation::validate_filename`. Both run the cases in
 * `crates/cmdr-fs/src/name_rules_cases.json`, so the two can't drift. `rules` is the
 * folder's (`getNameRules`), ❌ never a guess from the host OS. A refusal anywhere in the
 * name beats a warning; among warnings, the first is named.
 */
export function nameVerdict(name: string, rules: NameRules): NameVerdict {
  const universal = universalRefusal(name, rules)
  if (universal !== null) return { kind: 'refused', error: universal }

  let warning: PortabilityWarning | null = null
  for (const { tier, error, warn } of portabilityClasses(name, rules)) {
    if (tier === 'refuse') return { kind: 'refused', error }
    if (tier === 'warn') warning ??= warn
  }
  return warning === null ? { kind: 'ok' } : { kind: 'warned', warning }
}

/** ICU `select` value for a limit's unit: `bytes`, or `characters` for UTF-16 units. */
function unitWord(unit: LengthUnit): 'bytes' | 'characters' {
  return unit === 'bytes' ? 'bytes' : 'characters'
}

/** The words for a name refusal, the live check's and the backend's alike, so the two read the same. */
export function validationMessage(error: ValidationError, isDir: boolean): string {
  const kind = nameKind(isDir)
  switch (error.kind) {
    case 'empty':
      return tString('fileOperations.validation.empty', { kind })
    case 'dotName':
      return tString('fileOperations.validation.dotName', { kind })
    case 'disallowedCharacter':
      return tString('fileOperations.validation.disallowedCharacter', { kind, character: error.character })
    case 'nameTooLong':
      return tString('fileOperations.validation.nameLength', {
        kind,
        length: formatInteger(error.length),
        maxLength: formatInteger(error.max),
        max: error.max,
        unit: unitWord(error.unit),
      })
    case 'pathTooLong':
      return tString('fileOperations.validation.pathLength', {
        length: formatInteger(error.length),
        maxLength: formatInteger(error.max),
        max: error.max,
        unit: unitWord(error.unit),
      })
    case 'reservedName':
      return tString('fileOperations.validation.reservedName', { kind, name: error.name })
    case 'endsWithSpaceOrPeriod':
      return tString('fileOperations.validation.endsWithSpaceOrPeriod', { kind })
  }
}

/**
 * Whether a portability warning reaches Linux too: a stand-in on a Windows-format stick shows as
 * something else on Linux (verified on kernel 7.2.12, `docs/notes/windows-port/name-rules-on-a-mac-2026-11-18.md`).
 * A reserved name is only Windows' trouble, and a share's stand-ins map back for Linux's clients.
 * The mirror of `PortabilityWarning::reaches_linux`; `format` is the destination's.
 */
export function warningReachesLinux(warning: PortabilityWarning, format: NameFormat): boolean {
  return format === 'windows' && warning.kind !== 'reservedName'
}

/** The words for a name the destination takes but won't keep portable. */
export function portabilityMessage(warning: PortabilityWarning, rules: NameRules): string {
  const format = rules.format === 'smbShare' ? 'smbShare' : 'drive'
  switch (warning.kind) {
    case 'windowsCharacter':
      return tString('fileOperations.validation.notPortable.windowsCharacter', {
        format,
        character: warning.character,
      })
    case 'reservedName':
      return tString('fileOperations.validation.notPortable.reservedName', { format, name: warning.name })
    case 'endsWithSpaceOrPeriod':
      return tString('fileOperations.validation.notPortable.endsWithSpaceOrPeriod', { format })
  }
}

/**
 * Validates a (trimmed) file or folder name for a destination following `rules`: an error for
 * a refusal, a warning for a name it takes but won't keep portable (the operation goes ahead).
 */
export function validateName(name: string, isDir: boolean, rules: NameRules): ValidationResult {
  const verdict = nameVerdict(name, rules)
  switch (verdict.kind) {
    case 'ok':
      return OK_RESULT
    case 'refused':
      return { severity: 'error', message: validationMessage(verdict.error, isDir) }
    case 'warned':
      return { severity: 'warning', message: portabilityMessage(verdict.warning, rules) }
  }
}

/** Validates that a file or dir name is not empty after trimming. */
export function validateNotEmpty(name: string, isDir = false): ValidationResult {
  if (isBlank(name)) {
    return { severity: 'error', message: tString('fileOperations.validation.empty', { kind: nameKind(isDir) }) }
  }
  return OK_RESULT
}

/** Validates the full path's length against the folder's own path limit, in its unit (none: the backend answers). */
export function validatePathLength(parentPath: string, name: string, rules: NameRules): ValidationResult {
  if (rules.pathLimit === null) return OK_RESULT
  const length = exceededBy(joinPath(parentPath, name.trim()), rules.pathLimit)
  if (length === null) return OK_RESULT
  return {
    severity: 'error',
    message: validationMessage(
      { kind: 'pathTooLong', length, max: rules.pathLimit.max, unit: rules.pathLimit.unit },
      false,
    ),
  }
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

/** The limits a typed path is held to: the host's, or the destination's once its rules answer. */
export interface TypedPathLimits {
  name: LengthLimit
  /** `null`: the destination's backend answers for itself. */
  path: LengthLimit | null
}

/** A folder's rules as the limits a typed path in it is held to. */
export function limitsOf(rules: NameRules): TypedPathLimits {
  return { name: rules.nameLimit, path: rules.pathLimit }
}

/**
 * Validates a full directory path (not a filename). Checks structure only, not existence.
 * `limits` default to the host's; a caller that knows the destination's rules passes those
 * (`limitsOf`), so a deep share path isn't held to this Mac's `PATH_MAX`.
 */
export function validateDirectoryPath(path: string, limits: TypedPathLimits = hostLimits): ValidationResult {
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

  const totalLength = limits.path === null ? null : exceededBy(trimmed, limits.path)
  if (limits.path !== null && totalLength !== null) {
    return {
      severity: 'error',
      message: tString('fileOperations.validation.typedPathLength', {
        length: formatInteger(totalLength),
        maxLength: formatInteger(limits.path.max),
        max: limits.path.max,
        unit: unitWord(limits.path.unit),
      }),
    }
  }

  const segments = wire.split('/').filter((s) => s.length > 0)
  for (const segment of segments) {
    const segmentLength = exceededBy(segment, limits.name)
    if (segmentLength !== null) {
      return {
        severity: 'error',
        message: tString('fileOperations.validation.typedSegmentLength', {
          length: formatInteger(segmentLength),
          maxLength: formatInteger(limits.name.max),
          max: limits.name.max,
          unit: unitWord(limits.name.unit),
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

  const pathLen = validatePathLength(parentPath, newName, rules)
  if (pathLen.severity === 'error') return pathLen

  const extCheck = validateExtensionChange(originalName, newName, allowExtensionChanges, kind)
  if (extCheck.severity === 'error') return extCheck

  // Warning checks: a clash first, since it's about this folder; then a name that
  // won't keep portable (the rename goes ahead either way).
  const conflict = validateConflict(newName, siblingNames, originalName)
  if (conflict.severity === 'warning') return conflict

  return nameCheck
}
