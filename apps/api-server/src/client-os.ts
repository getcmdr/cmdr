/**
 * The OS a Cmdr build runs on, as the app and the release manifest spell it (`std::env::consts::OS`, and the `os` of
 * `latest.json`'s `downloads`). Shared by the write paths (`telemetry/`: the heartbeat, feedback, crash and error
 * reports, and `/download`) and the read paths (`admin/`), so it lives at the root.
 */

export const clientOses = ['macos', 'windows', 'linux'] as const

export type ClientOs = (typeof clientOses)[number]

export function isClientOs(value: unknown): value is ClientOs {
  return typeof value === 'string' && (clientOses as readonly string[]).includes(value)
}

/**
 * The `os` field an app sent, or null for absent (older clients, which serde writes as `null` or omits) and for any
 * value this server doesn't know. It's an enrichment, so an unknown value only unsets it: a 400 would make the app
 * resend a heartbeat forever, or lose a report over a field nobody needs to read it.
 */
export function parseClientOs(value: unknown): ClientOs | null {
  return isClientOs(value) ? value : null
}

/**
 * The OS of a stored row: its `os` column when set, else what the row's `os_version` says, for rows from clients
 * older than the field. Those came from three arms only (`apps/desktop/src-tauri/src/platform/`): macOS sends
 * `macOS 26.0` (and much older builds a bare `15.3.1`), Windows `Windows 11 Pro 24H2 (build …)`, and Linux its
 * distro's `PRETTY_NAME`, which can be anything, so everything else is Linux. Legacy data only: a new client always
 * sends `os`, so nothing new depends on this reading.
 */
export function osOfRow(row: { os: string | null; osVersion: string | null }): ClientOs {
  if (isClientOs(row.os)) return row.os
  const version = row.osVersion ?? ''
  if (/^(macOS\b|\d)/.test(version)) return 'macos'
  if (/^Windows\b/.test(version)) return 'windows'
  return 'linux'
}
