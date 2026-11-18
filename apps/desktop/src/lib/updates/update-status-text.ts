/**
 * Shared formatters for the update-check status and its failures. Used by both the Settings > Updates
 * section and the menu-triggered toast so the wording stays in sync.
 *
 * `formatUpdateStatus` returns `null` while a failure stands; callers render `describeUpdateFailure`
 * instead, with a follow-up "Send error report" link where `updateFailureOffersReport` says one is worth it.
 */
import { tString } from '#lib/intl/messages.svelte.js'
import { describeServerRequestFailure, serverRequestLogLevel } from '#lib/error-messages/server-request.js'
import type { ManagedUpdateOutcome, UnwritableUpdate, UpdateFailure } from './update-state.svelte'

export interface UpdateStatusReadable {
  status: 'idle' | 'checking' | 'downloading' | 'installing' | 'ready'
  failure: UpdateFailure | null
  managed: ManagedUpdateOutcome | null
  unwritable: UnwritableUpdate | null
  noBuild: boolean
  previousVersion: string | null
  nextVersion: string | null
}

/** The sentence for a check the organization's policy answered. */
function describeManagedOutcome(managed: ManagedUpdateOutcome): string {
  switch (managed.kind) {
    case 'updatesDisabledByPolicy':
      return tString('updates.status.managedOff')
    case 'heldByPolicy':
      return tString('updates.status.heldByPolicy', { available: managed.available, ceiling: managed.ceiling })
  }
}

/** The sentence for an update this user can't write into the bundle: replace it by hand, or ask. */
function describeUnwritableUpdate({ version, blocker }: UnwritableUpdate): string {
  return blocker === 'notWritableAdminUser'
    ? tString('updates.status.unwritableAdmin', { version })
    : tString('updates.status.unwritableStandard', { version })
}

export function formatUpdateStatus(state: UpdateStatusReadable): string | null {
  if (state.failure !== null) return null

  const prev = state.previousVersion ?? '?'
  const next = state.nextVersion ?? '?'

  switch (state.status) {
    case 'idle':
      if (state.managed !== null) return describeManagedOutcome(state.managed)
      if (state.unwritable !== null) return describeUnwritableUpdate(state.unwritable)
      if (state.noBuild) return tString('updates.status.noBuildForThisPlatform')
      // Two sub-cases share idle. If we just finished a successful check (we have a previousVersion
      // and no nextVersion), say "no updates found". Before any check has run, say nothing.
      if (state.previousVersion !== null && state.nextVersion === null) {
        return tString('updates.status.noUpdates', { version: prev })
      }
      return ''
    case 'checking':
      return tString('updates.status.checking')
    case 'downloading':
      return tString('updates.status.downloading', { next, prev })
    case 'installing':
      return tString('updates.status.installing', { next, prev })
    case 'ready':
      return tString('updates.status.ready', { next })
  }
}

/** The sentence for a check, download, or install that didn't finish. ❌ Never the backend's detail. */
export function describeUpdateFailure(failure: UpdateFailure): string {
  switch (failure.phase) {
    case 'check':
      return failure.request === null
        ? tString('updates.failure.checkUntyped')
        : tString('updates.failure.check', { reason: describeServerRequestFailure(failure.request) })
    case 'download':
      return tString('updates.failure.download')
    case 'install':
      return tString('updates.failure.install')
    case 'notGenuine':
      return tString('updates.failure.notGenuine')
  }
}

/**
 * Whether a failure is worth an error report: a download or install that didn't finish, or a check Cmdr's own server
 * refused or answered unreadably. Network trouble isn't (the report would ride the same broken connection), and an
 * untyped check can't tell the two apart, so neither offers one.
 */
export function updateFailureOffersReport(failure: UpdateFailure): boolean {
  if (failure.phase !== 'check') return true
  return failure.request !== null && serverRequestLogLevel(failure.request) === 'error'
}
