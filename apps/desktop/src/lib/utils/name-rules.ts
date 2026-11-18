/** The `NameRules` values the frontend holds before a folder's own answer lands. Pure. */

import type { NameRules } from '#lib/ipc/bindings.js'

/**
 * Only `/` and NUL, 255 bytes, no path limit: the answer while a folder's own rules are on
 * their way, and for a destination Cmdr can't see behind. Mirrors `NameRules::POSIX`.
 */
export const POSIX_RULES: NameRules = {
  format: 'posix',
  windowsCharacters: 'allow',
  reservedNames: 'allow',
  nameLimit: { max: 255, unit: 'bytes' },
  pathLimit: null,
}

/** Whether two rules sets answer every name the same. */
export function sameRules(a: NameRules, b: NameRules): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}
