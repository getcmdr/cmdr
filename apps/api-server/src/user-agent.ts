/**
 * User-Agent classification for `/download` rows. Shared by the write path (`telemetry/`, which stores
 * the family on each row) and the read path (`admin/funnel.ts`, which aggregates by it), so it lives
 * here rather than in either area: both would otherwise import from the other.
 */

import { isClientOs, type ClientOs } from './client-os'

/** The User-Agent family a download is classified into. See `classifyUaFamily`. */
export type UaFamily = 'human' | 'bot' | 'unknown'

/**
 * The markers that say a UA runs each OS, lowercase. Homebrew counts as macOS: the cask is the only Homebrew
 * path to Cmdr. ChromeOS (`CrOS`) says `X11`, which is right: its Linux container runs the AppImage.
 */
const osMarkers: Record<ClientOs, readonly string[]> = {
  macos: ['macintosh', 'mac os', 'homebrew'],
  windows: ['windows'],
  linux: ['linux', 'x11'],
}

/** Command-line downloaders, how scripted and manual installs fetch on every OS. */
const cliMarkers = ['curl', 'wget'] as const

/** Android says `Linux` in its UA but runs none of the builds. */
const androidMarker = 'android'

/**
 * Classify a `/download` hit's User-Agent into a coarse install-plausibility family, against the OS of
 * the file it fetched. The whole basis: a client on another OS can't install that file (a Windows
 * browser fetching a `.dmg`, a Mac fetching a `-setup.exe`).
 *
 * - `human` (a possible install): the UA names the file's own OS, or a CLI downloader (`curl` / `wget`).
 *   Checked first, so a UA that claims the right OS is never excluded. For a Linux file, Android is
 *   ruled out before this, since its UA also says `Linux`.
 * - `bot` (provably impossible): the UA names another OS, or Android. The one high-confidence exclusion.
 * - `unknown`: anything else, including a NULL/empty UA on rows captured before migration 0010. We can't
 *   tell, so callers must NOT exclude it from anything; they only exclude `bot`.
 *
 * Deliberately conservative: the scraper spoofs Mac browser UAs (lots of `Mozilla/5.0 (Macintosh; Intel
 * Mac OS X 10_15_7)` from China), so those land in `human` for a macOS file. We do NOT try to catch
 * spoofed bots by UA, and we never exclude by country: only the provably-impossible clients are dropped.
 * Pure (no I/O) so it's unit-testable.
 */
export function classifyUaFamily(userAgent: string | null | undefined, os: ClientOs): UaFamily {
  if (!userAgent) return 'unknown'
  const ua = userAgent.toLowerCase()
  const names = (markers: readonly string[]) => markers.some((marker) => ua.includes(marker))
  const android = ua.includes(androidMarker)
  if (os === 'linux' && android) return 'bot'
  if (names(osMarkers[os]) || names(cliMarkers)) return 'human'
  const otherOses = (Object.keys(osMarkers) as ClientOs[]).filter((other) => other !== os)
  if (android || otherOses.some((other) => names(osMarkers[other]))) return 'bot'
  return 'unknown'
}

/** The three families a download row can fall into. Guards the stored column against a bad value. */
const uaFamilies: ReadonlySet<string> = new Set<UaFamily>(['human', 'bot', 'unknown'])

/**
 * The family for one grouped download row: the value stored at write time when it's there, else the
 * classifier run over the raw UA (rows from before migration 0013) against the row's OS. Every row
 * from before the `os` column (migration 0033) is a macOS download, and the column's default says so;
 * an unknown value reads the same way. A row that has neither family nor UA, because the retention
 * sweep cleared the UA of a pre-0013 row, lands in `unknown`, which is never excluded from anything.
 */
export function resolveUaFamily(row: {
  uaFamily: string | null
  userAgent: string | null
  os: string | null
}): UaFamily {
  if (row.uaFamily !== null && uaFamilies.has(row.uaFamily)) return row.uaFamily as UaFamily
  return classifyUaFamily(row.userAgent, isClientOs(row.os) ? row.os : 'macos')
}
