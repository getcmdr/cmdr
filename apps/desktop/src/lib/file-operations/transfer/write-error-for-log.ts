/**
 * A write error as the frontend's log line carries it: every typed field, ❌ never the OS's words.
 *
 * `message` is OS, server, or tool prose, and it names items (macOS: “Test App.app” couldn’t be
 * moved to the trash…). The backend's `write_operations` WARN line (`operations_host/terminal_effects.rs`
 * `warn_failure`) is its ONE record: it logs the same error with each value in that prose marked,
 * so a report hashes exactly the name. This side only has the plain JSON, where a name with no key
 * beside it reads as prose and ships in clear (ERR-RB2XQ, cmdr-reports#122). Every frontend
 * write-error comes off a backend `write-error` event, so nothing is lost: the op id joins the two lines.
 */

import type { WriteOperationError } from '../write-operation-types'

/** The prose key, at any depth (`cause` nests a whole error). Mirrors the one `Detail` key of the backend's `log_field`. */
const OS_WORDS_KEY = 'message'

export function writeErrorForLog(error: WriteOperationError): unknown {
  return withoutOsWords(error)
}

function withoutOsWords(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(withoutOsWords)
  if (typeof value !== 'object' || value === null) return value
  return Object.fromEntries(
    Object.entries(value)
      .filter(([key]) => key !== OS_WORDS_KEY)
      .map(([key, nested]) => [key, withoutOsWords(nested)]),
  )
}
