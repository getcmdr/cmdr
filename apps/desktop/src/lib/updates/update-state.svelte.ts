/**
 * Module-level singleton state for the updater. Lives here (not in `updater.svelte.ts`) so toast
 * components can read it without forming an import cycle: the toast components import this module,
 * `updater.svelte.ts` imports both this module and the toast components, and the cycle stays
 * one-way.
 */

import type { BundleWriteBlocker } from '#lib/tauri-commands/index.js'
import type { SecurityNotice, ServerRequestError, UpdateCheckOutcome } from '#lib/ipc/bindings.js'

/** The blockers about WHERE the bundle sits: the "move Cmdr to Applications" dialog's. */
export type LocationBlocker = Extract<BundleWriteBlocker, 'translocated' | 'readOnlyVolume'>

/** The blockers about WHO may write the bundle: the unwritable-bundle toast's. */
export type PermissionBlocker = Exclude<BundleWriteBlocker, LocationBlocker>

export function isPermissionBlocker(blocker: BundleWriteBlocker): blocker is PermissionBlocker {
  return blocker === 'notWritableAdminUser' || blocker === 'notWritableStandardUser'
}

/** An update this install found but can't write into its own bundle, and why. */
export interface UnwritableUpdate {
  version: string
  blocker: PermissionBlocker
}

/** The update the last check found and staged. The backend keeps where it came from; the frontend needs the version. */
export interface UpdateInfo {
  version: string
}

/**
 * A check the organization's policy answered: updates are off, or a release is held back by the ceiling. The backend's
 * own outcome, rendered as is. ❌ Not a failure: no error copy, no report link.
 */
export type ManagedUpdateOutcome = Extract<UpdateCheckOutcome, { kind: 'updatesDisabledByPolicy' | 'heldByPolicy' }>

/**
 * Why the last check, download, or install didn't finish, as a typed value the surfaces word from the catalog
 * (`update-status-text.ts`). ❌ Never a message.
 *
 * `request` is the macOS check's typed failure, or `null` when the check failed untyped (the Tauri plugin on other
 * platforms, or a broken IPC bridge).
 */
export type UpdateFailure =
  | { phase: 'check'; request: ServerRequestError | null }
  | { phase: 'download' }
  | { phase: 'install' }
  /**
   * The offered update didn't carry Cmdr's signature (minisign on the archive, or the Developer ID on the bundle), or
   * is a version that didn't earlier this session, so nothing was installed. Worded like its security notice
   * (`#lib/security-notices`), ❌ never "try again later".
   */
  | { phase: 'notGenuine' }

/** Who updates a copy Cmdr can't update itself: the Microsoft Store, or a newer package. */
export type UpdatesElsewhere = 'store' | 'package'

export interface UpdateState {
  status: 'idle' | 'checking' | 'downloading' | 'installing' | 'ready'
  update: UpdateInfo | null
  /** Why the last attempt didn't finish, or `null`. Cleared when a check starts. */
  failure: UpdateFailure | null
  /** The organization's answer to the last check, or `null`. Cleared when a check starts. */
  managed: ManagedUpdateOutcome | null
  /**
   * An update the last check found that this user can't write into the bundle (a root-owned `.pkg` install), or
   * `null`. A terminal phase like `managed`, ❌ not a failure: Settings words it from the catalog. Cleared when a check
   * starts.
   */
  unwritable: UnwritableUpdate | null
  /**
   * The last check found a newer release with no build for this platform in `latest.json`, so this build can't update
   * for now. A terminal phase like `managed`, ❌ not a failure and ❌ never "up to date": Settings and the check toast say
   * this build can't check right now. Cleared when a check starts.
   */
  noBuild: boolean
  /**
   * This copy is an MSIX package, which never updates itself, so Cmdr's own updater never checks: `'store'` when the
   * Microsoft Store signed it (the Store updates it), `'package'` for any other package (a newer package updates it).
   * A terminal phase like `managed`, ❌ not a failure and ❌ never "up to date": Settings and the check toast say where
   * updates come from, and only `'store'` names the Store. Cleared when a check starts; the backend answers the same
   * every time.
   */
  updatesElsewhere: UpdatesElsewhere | null
  /** Version the user is currently running. Set when `checking` starts. */
  previousVersion: string | null
  /** Version we're moving to. Set when an update is found. Cleared on `idle`. */
  nextVersion: string | null
  /**
   * What the staged update fixes, security-wise, or `null` when nothing the running build lacks. Set from the install's
   * own answer (the staged bundle's signed list, never `latest.json`) and REPLACED when a newer build supersedes it: that
   * bundle's list is cumulative, so its answer is the whole truth.
   */
  securityNotice: SecurityNotice | null
}

export const updateState = $state<UpdateState>({
  status: 'idle',
  update: null,
  failure: null,
  managed: null,
  unwritable: null,
  noBuild: false,
  updatesElsewhere: null,
  previousVersion: null,
  nextVersion: null,
  securityNotice: null,
})

/**
 * The "move Cmdr to Applications" nudge. `blocker` is non-null exactly while the dialog is up.
 *
 * Lives beside `updateState` rather than inside it: it isn't a phase of the update state machine,
 * it's the one thing we can do for an install that will never finish one. `+layout.svelte` mounts
 * the dialog off this, and `updater.svelte.ts` raises it.
 */
export const updateBlockerNotice = $state<{ blocker: LocationBlocker | null }>({ blocker: null })
