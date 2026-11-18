/**
 * The frontend half of the app path's lossless escape (`cmdr_fs::app_path`, its `escape.rs`).
 *
 * A native name that isn't valid Unicode text reaches the frontend with each rejected unit
 * spelled as ONE plane-16 private-use code point: a byte escape (U+10FE80–U+10FEFF, from a
 * Linux disk, SFTP, NFS) or a unit escape (U+10E800–U+10EFFF, an unpaired surrogate from a
 * Windows disk or SMB). In a JS string that's one surrogate pair, never a lone surrogate, so
 * `JSON.parse`, `TextEncoder`, and `encodeURIComponent` all take it.
 *
 * The backend spells a row's `path` and `name` this way, and the frontend sends them back
 * byte for byte: an escape is an ADDRESS, ❌ never respelled here. Only what a person SEES
 * changes: every escape renders as U+FFFD (`shownName`, and `displayPath` over any text),
 * exactly what the lossy spelling always showed. Splitting on `/` never cuts one.
 */

/** The first byte escape: byte `b` is `BYTE_BASE + b`, for the bytes `0x80`–`0xFF`. */
const BYTE_ESCAPE_FIRST = 0x10fe80
const BYTE_ESCAPE_LAST = 0x10feff
/** The unit escapes: surrogate `u` is `0x10e800 + (u - 0xd800)`. */
const UNIT_ESCAPE_FIRST = 0x10e800
const UNIT_ESCAPE_LAST = 0x10efff

/** Both alphabets, as one regex over code points. */
const ESCAPE = /[\u{10E800}-\u{10EFFF}\u{10FE80}-\u{10FEFF}]/gu

/** A plane-16 code point's high surrogate, the cheap pre-check before the regex. */
const PLANE_16_HIGH_FIRST = 0xdbc0

/** Whether the code point `cp` is one of the escape alphabets'. */
export function isEscape(cp: number): boolean {
  return (cp >= BYTE_ESCAPE_FIRST && cp <= BYTE_ESCAPE_LAST) || (cp >= UNIT_ESCAPE_FIRST && cp <= UNIT_ESCAPE_LAST)
}

/** Whether `text` may hold an escape: only a plane-16 high surrogate can start one. */
function mayHoldEscape(text: string): boolean {
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) >= PLANE_16_HIGH_FIRST && text.charCodeAt(i) <= 0xdbff) return true
  }
  return false
}

/**
 * `text` as a person reads it: every escape as U+FFFD. Any text (a name, a path, a whole
 * message), since a plane-16 escape never means anything in prose. Text without one comes
 * back as is.
 */
export function withoutEscapes(text: string): string {
  if (!mayHoldEscape(text)) return text
  return text.replace(ESCAPE, '�')
}

/**
 * A row's name as a person reads it: the listing's name cells, the rename prefill and its
 * "unchanged" check, search results, the selection summary, drag images. ❌ Never send it
 * back as an address: the row's `name` and `path` are the addresses.
 */
export function shownName(name: string): string {
  return withoutEscapes(name)
}

/** How a native namer counts a name: bytes (a Unix disk, SFTP, NFS) or UTF-16 units (Windows, SMB). */
export type NativeUnit = 'bytes' | 'units'

/**
 * How long `name` is natively, in `unit`: one per escape (it stands for one byte or one
 * unit), the encoded length of everything else. What a 255-byte or 255-unit name limit
 * measures; ❌ never `TextEncoder` over the escaped name, which counts four bytes per escape.
 */
export function nativeNameLength(name: string, unit: NativeUnit): number {
  let length = 0
  for (const char of name) {
    const cp = char.codePointAt(0) ?? 0
    if (isEscape(cp)) length += 1
    else if (unit === 'units') length += cp > 0xffff ? 2 : 1
    else length += cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4
  }
  return length
}
