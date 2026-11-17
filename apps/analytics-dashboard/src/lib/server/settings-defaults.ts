/**
 * Turns the heartbeat's sparse config shape into settings adoption.
 *
 * The app persists only settings a user explicitly CHANGED, so the config shape on the wire carries
 * deviation and never adoption: a key absent from an install's row means EITHER "still on the
 * default" OR "this setting didn't exist in that build", and the row can't tell them apart. The
 * defaults manifest can, because it records what each release shipped:
 *
 *     effective = override ?? defaults[newest entry at or below the install's app version][key]
 *
 * The second half is the part that has to be right. A key missing from the resolved entry means the
 * setting did not exist in that build, so that install drops OUT of the denominator rather than
 * counting as a default. Skip that and every new setting silently corrupts its own adoption number
 * for every older install still running: they'd all be scored as "on the default" for a setting they
 * never had.
 *
 * The manifest itself: `apps/desktop/scripts/gen-analytics-defaults-lib.ts`, pinned to the settings
 * registry by the `settings-defaults` check.
 */

import manifestJson from './settings-defaults.gen.json'

/** A setting value as it survives the JSON round trip. */
export type SettingValue = boolean | number | string

interface DefaultsManifest {
  /**
   * Keys the desktop registry tags `kind: 'state'` (remembered UI or onboarding state, not a preference),
   * plus retired ones old installs still carry. Server-side only, like the rest of the manifest.
   */
  stateKeys: string[]
  versions: Record<string, Record<string, SettingValue>>
}

const manifest = manifestJson as DefaultsManifest

/** One row of `/admin/config-shape`'s `values`: how many installs reported this exact value. */
export interface ConfigShapeValueRow {
  appVersion: string
  key: string
  /** SQLite's `json_each.type`, which is how a JSON boolean survives D1 flattening it to 1 / 0. */
  type: string
  value: string | number
  installs: number
}

/** One row of `/admin/config-shape`'s `installs`: the per-version denominator. */
export interface ConfigShapeInstallRow {
  appVersion: string
  installs: number
}

/** One effective value of one setting, across every install whose build has that setting. */
export interface AdoptionValue {
  /** Rendered for display; the raw value is only ever compared, never shown. */
  label: string
  installs: number
  isDefault: boolean
}

export interface SettingAdoption {
  key: string
  /** `state` for remembered UI or onboarding state (the registry's `kind: 'state'`), else `preference`. */
  kind: 'preference' | 'state'
  /** Installs whose build actually HAS this setting. The only honest denominator. */
  eligible: number
  /**
   * Installs running the default their OWN version shipped, whether they never touched the setting
   * or set it back. Scored per version, so a default that moved mid-window doesn't read as everyone
   * having changed it.
   */
  onDefault: number
  /** The default, or null when it isn't the same across the versions in the window. */
  defaultLabel: string | null
  /** The default the newest version in the window ships. */
  latestDefaultLabel: string
  /**
   * The manifest version where the default last moved to `latestDefaultLabel`, or null when it has been
   * that since the setting arrived.
   */
  defaultMovedIn: string | null
  /** Effective values, most common first. */
  values: AdoptionValue[]
}

export interface SettingsAdoption {
  settings: SettingAdoption[]
  /** Installs whose latest heartbeat carried a config shape, whatever version they're on. */
  totalInstalls: number
  /**
   * Installs on an app version older than the manifest's first entry, so nothing at all can be said
   * about their settings. Surfaced rather than folded in: a silent drop is how a denominator lies.
   */
  unresolvedInstalls: number
}

/** Oldest first. Every Cmdr version is `MAJOR.MINOR.PATCH`. */
function compareVersionsAsc(a: string, b: string): number {
  const pa = a.split('.').map(Number)
  const pb = b.split('.').map(Number)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const diff = (pa[i] ?? 0) - (pb[i] ?? 0)
    if (diff !== 0) return diff
  }
  return 0
}

/**
 * The defaults that applied to `version`: the newest manifest entry at or below it, or null when
 * the manifest starts after that version.
 */
export function defaultsForVersion(
  version: string,
  versions: Record<string, Record<string, SettingValue>> = manifest.versions,
): Record<string, SettingValue> | null {
  let best: string | null = null
  for (const candidate of Object.keys(versions)) {
    if (compareVersionsAsc(candidate, version) > 0) continue
    if (best === null || compareVersionsAsc(candidate, best) > 0) best = candidate
  }
  return best === null ? null : versions[best]
}

/** Rebuilds a value's real type from the `type` SQLite reported beside it. */
export function decodeValue(row: Pick<ConfigShapeValueRow, 'type' | 'value'>): SettingValue | null {
  switch (row.type) {
    case 'true':
      return true
    case 'false':
      return false
    case 'integer':
    case 'real':
      return Number(row.value)
    case 'text':
      return String(row.value)
    default:
      // `null`, `array`, and `object` never reach the config shape (the backend's allowlist drops
      // them), so anything else is a shape we don't model and shouldn't guess at.
      return null
  }
}

/** How a value reads in the UI. Booleans get words, since "true" in a table is noise. */
export function labelValue(value: SettingValue): string {
  if (value === true) return 'on'
  if (value === false) return 'off'
  return String(value)
}

/** Effective value (JSON-encoded, so a bucket key is exact) to the installs running it. */
type ValueTally = Map<string, { value: SettingValue; installs: number }>

/** Per key: what installs are running, and which defaults were in play across the window. */
interface AdoptionTally {
  values: Map<string, ValueTally>
  defaultsSeen: Map<string, Set<string>>
  /** The newest app version in the window whose build has the key. */
  newestVersion: Map<string, string>
  /**
   * Installs running the default THEIR version shipped. Counted per version rather than derived
   * from the value buckets, because a default that moved mid-window has no single bucket to point
   * at, and deriving it would rank such a setting as "everybody changed it" when nobody did.
   */
  onDefault: Map<string, number>
}

function addTally(tally: ValueTally, value: SettingValue, installs: number): void {
  if (installs === 0) return
  const bucketKey = JSON.stringify(value)
  const bucket = tally.get(bucketKey)
  if (bucket === undefined) tally.set(bucketKey, { value, installs })
  else bucket.installs += installs
}

/** Groups the raw value rows by version and key, decoding each value back to its real type. */
function indexOverrides(valueRows: ConfigShapeValueRow[]): Map<string, Map<string, ValueTally>> {
  const byVersion = new Map<string, Map<string, ValueTally>>()
  for (const row of valueRows) {
    const value = decodeValue(row)
    if (value === null) continue
    let forVersion = byVersion.get(row.appVersion)
    if (forVersion === undefined) {
      forVersion = new Map<string, ValueTally>()
      byVersion.set(row.appVersion, forVersion)
    }
    let forKey = forVersion.get(row.key)
    if (forKey === undefined) {
      forKey = new Map()
      forVersion.set(row.key, forKey)
    }
    addTally(forKey, value, row.installs)
  }
  return byVersion
}

/**
 * Folds one app version's installs into the running tally, scoring each setting against the
 * defaults THAT version shipped.
 *
 * Only the keys in `defaults` are touched, which is where the "the setting didn't exist yet" rule
 * lives: a key absent from this version's entry contributes neither a value nor a denominator, and
 * an override row for it (a stale entry left in someone's settings.json by an older build) is inert
 * and ignored.
 */
function accumulateVersion(
  tally: AdoptionTally,
  appVersion: string,
  defaults: Record<string, SettingValue>,
  overrides: Map<string, ValueTally>,
  installs: number,
): void {
  for (const [key, defaultValue] of Object.entries(defaults)) {
    let values = tally.values.get(key)
    if (values === undefined) {
      values = new Map()
      tally.values.set(key, values)
    }
    let seen = tally.defaultsSeen.get(key)
    if (seen === undefined) {
      seen = new Set()
      tally.defaultsSeen.set(key, seen)
    }
    seen.add(JSON.stringify(defaultValue))
    const newest = tally.newestVersion.get(key)
    if (newest === undefined || compareVersionsAsc(appVersion, newest) > 0) tally.newestVersion.set(key, appVersion)

    const defaultKey = JSON.stringify(defaultValue)
    let explicit = 0
    let atDefault = 0
    for (const bucket of overrides.get(key)?.values() ?? []) {
      explicit += bucket.installs
      if (JSON.stringify(bucket.value) === defaultKey) atDefault += bucket.installs
      addTally(values, bucket.value, bucket.installs)
    }
    // Whoever set nothing is running the default. Clamped at zero so a value row whose install fell
    // outside the totals can never produce a negative count.
    const implicit = Math.max(0, installs - explicit)
    addTally(values, defaultValue, implicit)
    tally.onDefault.set(key, (tally.onDefault.get(key) ?? 0) + implicit + atDefault)
  }
}

/**
 * The default `version` ships for `key`, and the manifest version it last moved to that value in (null
 * when the key has had it since it arrived).
 */
function latestDefault(
  key: string,
  version: string,
  versions: Record<string, Record<string, SettingValue>>,
): { value: SettingValue; movedIn: string | null } | null {
  const entries = Object.keys(versions)
    .filter((candidate) => compareVersionsAsc(candidate, version) <= 0)
    .sort(compareVersionsAsc)
  const current = entries.length > 0 ? versions[entries[entries.length - 1]][key] : undefined
  if (current === undefined) return null
  const currentKey = JSON.stringify(current)
  for (let i = entries.length - 1; i > 0; i--) {
    const previous = versions[entries[i - 1]]
    if (!(key in previous)) return { value: current, movedIn: null }
    if (JSON.stringify(previous[key]) !== currentKey) return { value: current, movedIn: entries[i] }
  }
  return { value: current, movedIn: null }
}

/** Turns one key's tally into its rendered breakdown. */
function summarizeSetting(
  key: string,
  kind: SettingAdoption['kind'],
  values: ValueTally,
  defaultsSeen: Set<string>,
  onDefault: number,
  latest: { value: SettingValue; movedIn: string | null },
): SettingAdoption {
  // A default that moved mid-window has no single value to compare against, so the breakdown says
  // so rather than picking one release's answer and calling everyone else a deviation.
  const defaultKey = defaultsSeen.size === 1 ? [...defaultsSeen][0] : null
  const breakdown = [...values.values()]
    .filter((entry) => entry.installs > 0)
    .map((entry) => ({
      label: labelValue(entry.value),
      installs: entry.installs,
      isDefault: defaultKey !== null && JSON.stringify(entry.value) === defaultKey,
    }))
    .sort((a, b) => b.installs - a.installs || a.label.localeCompare(b.label))

  return {
    key,
    kind,
    eligible: breakdown.reduce((sum, entry) => sum + entry.installs, 0),
    onDefault,
    defaultLabel: defaultKey === null ? null : labelValue(JSON.parse(defaultKey) as SettingValue),
    latestDefaultLabel: labelValue(latest.value),
    defaultMovedIn: latest.movedIn,
    values: breakdown,
  }
}

/**
 * Resolves every setting the manifest knows about into an adoption breakdown.
 *
 * Rows for a key the resolved entry doesn't list are ignored on purpose: the setting didn't exist in
 * that build, so a value left in that install's `settings.json` by an older version is inert and
 * counting it would be inventing a user.
 *
 * Each setting is stamped `state` or `preference` from the manifest's `stateKeys`, which the desktop
 * registry's typed `kind` decides: never from the key's name.
 */
export function resolveAdoption(
  installRows: ConfigShapeInstallRow[],
  valueRows: ConfigShapeValueRow[],
  versions: Record<string, Record<string, SettingValue>> = manifest.versions,
  stateKeys: readonly string[] = manifest.stateKeys,
): SettingsAdoption {
  const overridesByVersion = indexOverrides(valueRows)
  const state = new Set(stateKeys)
  const tally: AdoptionTally = {
    values: new Map(),
    defaultsSeen: new Map(),
    newestVersion: new Map(),
    onDefault: new Map(),
  }
  let totalInstalls = 0
  let unresolvedInstalls = 0

  for (const { appVersion, installs } of installRows) {
    totalInstalls += installs
    const defaults = defaultsForVersion(appVersion, versions)
    if (defaults === null) {
      unresolvedInstalls += installs
      continue
    }
    accumulateVersion(
      tally,
      appVersion,
      defaults,
      overridesByVersion.get(appVersion) ?? new Map<string, ValueTally>(),
      installs,
    )
  }

  const settings = [...tally.values.entries()]
    .flatMap(([key, values]) => {
      // Every tallied key has a newest version with that key in its defaults, so `latest` always resolves.
      const newest = tally.newestVersion.get(key)
      const latest = newest === undefined ? null : latestDefault(key, newest, versions)
      if (latest === null) return []
      return [
        summarizeSetting(
          key,
          state.has(key) ? 'state' : 'preference',
          values,
          tally.defaultsSeen.get(key) ?? new Set<string>(),
          tally.onDefault.get(key) ?? 0,
          latest,
        ),
      ]
    })
    .sort((a, b) => a.key.localeCompare(b.key))

  return { settings, totalInstalls, unresolvedInstalls }
}
