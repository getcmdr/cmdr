/**
 * The words for a typed `EjectError`: what an eject or a network-share
 * disconnect says to the person who asked for it.
 *
 * Classification is the backend's (`file_system/volume/eject/mod.rs`); the words are
 * all here, pulled from the `errors.eject.*` catalog so every locale gets its
 * own. Same split as the mutation path
 * (`#lib/file-operations/mutation-error-messages.ts`);
 * `docs/guides/error-handling.md` is the map.
 *
 * These render as ONE plain-text line inside a toast, so each message is a
 * single sentence with no markdown and no `{@html}`. Copy is pulled via
 * `getMessage()` (a RAW catalog lookup, never ICU `t()`) for the same reason the
 * mutation factory does it: the surrounding toast interpolates uncontrolled
 * volume names, whose apostrophes and braces collide with ICU grammar.
 *
 * ❌ `diskutil`'s own stderr is NEVER the message. {@link ejectTechnicalDetail}
 * returns it separately, for the log.
 */
import type { EjectError, HolderScan, VolumeHolder } from '#lib/ipc/bindings.js'
import { getMessage } from '#lib/intl/messages.svelte.js'
import { formatConjunctionList } from '#lib/intl/list-format.js'
import { formatInteger } from '#lib/intl/number-format.js'
import type { MessageKey } from '#lib/intl/keys.gen.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { asEjectError } from './eject-error'

const log = getAppLogger('eject')

/** Raw catalog lookup for an `errors.eject.*` key. */
function raw(key: MessageKey): string {
  return getMessage(key)
}

/** Substitutes `{token}` placeholders in a raw catalog value with runtime strings. */
function interpolate(template: string, params: Record<string, string>): string {
  let out = template
  for (const [name, value] of Object.entries(params)) out = out.replaceAll(`{${name}}`, value)
  return out
}

/** How many holder names a refusal spells out before it says "other apps" (or "others", for processes). */
const NAMES_SPELLED_OUT = 3

/**
 * The names an `App` or `Tool` holder contributes, deduped and in the order the
 * scan saw them.
 *
 * `App` and `Tool` word the same: what a person can act on is the NAME, and both
 * carry one they'd recognize (an app's display name, a tool's executable name).
 * Two processes of one app are one name, so a helper-heavy app doesn't fill the
 * toast with itself.
 */
function actionableNames(named: readonly VolumeHolder[]): string[] {
  return namesOf(named, (kind) => kind === 'app' || kind === 'tool')
}

/** The deduped names of the holders whose kind `keep` accepts, in the order the scan saw them. */
function namesOf(named: readonly VolumeHolder[], keep: (kind: VolumeHolder['kind']) => boolean): string[] {
  const seen = new Set<string>()
  for (const holder of named) {
    if (keep(holder.kind)) seen.add(holder.name)
  }
  return [...seen]
}

/**
 * `names` capped at {@link NAMES_SPELLED_OUT}, with `rest` standing in for the
 * others, joined by the UI language's own list rules.
 */
function spellOut(names: readonly string[], rest: MessageKey): string {
  const listed = names.length > NAMES_SPELLED_OUT ? [...names.slice(0, NAMES_SPELLED_OUT), raw(rest)] : names
  return formatConjunctionList(listed)
}

/**
 * The sentence for holders that were named but never classified. ❗ Worded as
 * PROCESSES, ❌ never as apps or tools: the name is an executable's, which is all
 * the scan could read.
 */
function wordUnclassified(names: readonly string[]): string {
  if (names.length === 1) return interpolate(raw('errors.eject.unmountRefusedByProcess'), { process: names[0] })
  return interpolate(raw('errors.eject.unmountRefusedByProcesses'), {
    countText: formatInteger(names.length),
    processes: spellOut(names, 'errors.eject.otherProcesses'),
  })
}

/**
 * The one sentence a refused unmount says, given who the backend found holding
 * the drive.
 *
 * Precedence, most actionable first: a named app or tool (something the person
 * can go and close), then a disk image (which has to be ejected first), then
 * Cmdr itself (a bug), then macOS (nothing to do but wait), then the bare
 * process names of `Unclassified` holders, then the empty arms.
 *
 * ❗ An `Unclassified` holder is a real answer meaning "named, but nothing said
 * what kind" (the scan's time budget ran out, or a code signature wouldn't read),
 * so it is worded as a process and ❌ never as an app or a tool.
 *
 * ❗ The two `HolderScan` arms word the same while they carry names, and differ
 * only when empty. `Incomplete` means the scan couldn't cover every mount, so its
 * emptiness says nothing and takes the unnamed fallback. A `Complete` scan with no
 * names means no SAME-UID process held the drive, so the holder is one the walk
 * can't see (a root daemon) and the sentence is macOS's "wait". Neither ever says
 * "nothing is using this drive".
 */
export function wordUnmountRefusal(holders: HolderScan): string {
  const named = holders.named
  const names = actionableNames(named)
  if (names.length === 1) return interpolate(raw('errors.eject.unmountRefusedByApp'), { app: names[0] })
  if (names.length > 1) {
    return interpolate(raw('errors.eject.unmountRefusedByApps'), { apps: spellOut(names, 'errors.eject.otherApps') })
  }
  if (named.some((h) => h.kind === 'diskImage')) return raw('errors.eject.unmountRefusedByDiskImage')
  if (named.some((h) => h.kind === 'cmdr')) return raw('errors.eject.unmountRefusedByCmdr')
  if (named.some((h) => h.kind === 'system')) return raw('errors.eject.unmountRefusedBySystem')
  const unclassified = namesOf(named, (kind) => kind === 'unclassified')
  if (unclassified.length > 0) return wordUnclassified(unclassified)
  // The walk sees same-uid processes only, so a complete scan that named nobody
  // was refused by a holder it can't see: almost always a root daemon (ERR-ZPABB).
  if (holders.type === 'complete') return raw('errors.eject.unmountRefusedBySystem')
  return raw('errors.eject.unmountRefused')
}

/**
 * One renderer per `EjectError` variant.
 *
 * A record rather than a `switch`: the mapped type still demands every variant,
 * so a backend that grows one stops the frontend compiling until it has words.
 */
const EJECT_MESSAGE: { [K in EjectError['type']]: (error: Extract<EjectError, { type: K }>) => string } = {
  busy: () => raw('errors.eject.busy'),
  volumeNotFound: () => raw('errors.eject.volumeNotFound'),
  notEjectable: () => raw('errors.eject.notEjectable'),
  notAnSmbVolume: () => raw('errors.eject.notAnSmbVolume'),
  remoteNotConnected: () => raw('errors.eject.remoteNotConnected'),
  deviceDisconnectRefused: () => raw('errors.eject.deviceDisconnectRefused'),
  unmountRefused: (e) => wordUnmountRefusal(e.holders),
  // The same names and the same advice; the toast around it says nothing was unmounted.
  heldBeforeEject: (e) => wordUnmountRefusal(e.holders),
  // Nobody holds the drive: what it needs is rights, ❌ never "close your files".
  needsAdministrator: () => raw('errors.eject.needsAdministrator'),
  timedOut: () => raw('errors.eject.timedOut'),
  // Nothing was unmounted, so it must NOT share `timedOut`'s "may still eject".
  notResponding: () => raw('errors.eject.notResponding'),
  // The single honest fallback. ❌ `detail` is never the message; it goes to
  // `ejectTechnicalDetail()`.
  unexpected: () => raw('errors.eject.unexpected'),
}

/** The one sentence an `EjectError` says. */
export function renderEjectError(error: EjectError): string {
  const render = EJECT_MESSAGE[error.type] as (error: EjectError) => string
  return render(error)
}

/**
 * What a refused eject's toast offers: `false` for no "Send error report…", `undefined` for the
 * toast's usual offer.
 *
 * ❗ A refusal that says who to close, or that names a cause someone can act on, isn't broken, so
 * offering a report reads as if it were. The offer stays where Cmdr itself held the drive (its
 * sentence asks for one), where the scan named nobody, and for everything nothing classifies.
 */
export function ejectRefusalReport(error: unknown): false | undefined {
  const typed = asEjectError(error)
  if (!typed) return undefined
  switch (typed.type) {
    case 'busy':
    case 'volumeNotFound':
    case 'notEjectable':
    case 'notAnSmbVolume':
    case 'remoteNotConnected':
    case 'needsAdministrator':
      return false
    case 'unmountRefused':
    case 'heldBeforeEject':
      return holdersExplainIt(typed.holders) ? false : undefined
    case 'deviceDisconnectRefused':
    case 'timedOut':
    case 'notResponding':
    case 'unexpected':
      return undefined
  }
}

/** Whether {@link wordUnmountRefusal}'s sentence says who holds the drive, and isn't Cmdr's own. */
function holdersExplainIt(holders: HolderScan): boolean {
  const named = holders.named
  if (actionableNames(named).length > 0 || named.some((h) => h.kind === 'diskImage')) return true
  if (named.some((h) => h.kind === 'cmdr')) return false
  // A `complete` scan that named nobody words the same "macOS, wait" as a `system` holder.
  return named.length > 0 || holders.type === 'complete'
}

/** `EjectError` variants that carry their own free-text `detail` field. */
const DETAIL_BEARING_VARIANTS = new Set<EjectError['type']>([
  'unmountRefused',
  'deviceDisconnectRefused',
  'needsAdministrator',
  'unexpected',
])

/**
 * The backend's own words for this refusal (usually `diskutil`'s stderr, which
 * often names the process holding the drive), for the log.
 *
 * ❌ Never render this as the message: it's untranslated diagnostic text.
 */
export function ejectTechnicalDetail(error: EjectError): string | null {
  return DETAIL_BEARING_VARIANTS.has(error.type) ? (error as { detail: string }).detail : null
}

/**
 * The one sentence a caught eject / disconnect refusal says, with the backend's
 * technical detail routed to the log.
 *
 * The three toasts that word an eject share this so none of them can drift into
 * putting `diskutil`'s untranslated stderr in front of a person. A value that
 * isn't an `EjectError` at all (the IPC transport itself broke) reads as the
 * same honest fallback, with the raw value logged.
 */
export function wordEjectRefusal(error: unknown): string {
  const typed = asEjectError(error)
  if (!typed) {
    log.warn('Eject refused with an untyped value: {error}', { error })
    return raw('errors.eject.unexpected')
  }
  // The reason alone. The technical detail is the tool's stderr (`diskutil` names the volume in
  // it), which here would be unmarked prose with no key beside it; the backend's `eject` WARN
  // logs that same stderr beside the marked mount path, whose leaf a report scrubs from it.
  log.warn('Eject refused: {reason}', { reason: typed.type })
  warnIfCmdrHeldTheDrive(typed)
  return renderEjectError(typed)
}

/**
 * A line of its own whenever Cmdr is among the holders.
 *
 * ❗ Keyed on PRESENCE, not on winning the precedence: an app beside a Cmdr
 * holder rightly gets the sentence, but Cmdr holding a drive it's trying to let
 * go of is a bug worth seeing whichever sentence won. The backend logs the whole
 * `HolderScan` at `info` in `name_the_holders`; this is the line that says it
 * was us.
 */
function warnIfCmdrHeldTheDrive(error: EjectError): void {
  if (error.type !== 'unmountRefused' && error.type !== 'heldBeforeEject') return
  const ours = error.holders.named.filter((h) => h.kind === 'cmdr')
  if (ours.length === 0) return
  log.warn('Cmdr itself held the drive when the unmount was refused: {pids}', {
    pids: ours.map((h) => h.pid).join(', '),
  })
}
