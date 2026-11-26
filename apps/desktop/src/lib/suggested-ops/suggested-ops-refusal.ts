/**
 * The words under a suggestion whose approval refused to start.
 *
 * A refusal the write engine gave (a phone nobody connected, a drive that left, no space) is
 * the same `WriteOperationError` a clicked operation would have hit, so it reads through the
 * same `errors.write.*` pipeline (`../file-operations/transfer/transfer-error-messages.ts`):
 * the suggestion says exactly what the transfer dialog would. A server card's refused save is
 * the same outcome the servers sheet words, so it reads through `../servers/connect-refusals.ts`.
 * Only the refusals that exist for suggestions alone carry their own copy.
 */

import { tString } from '#lib/intl/messages.svelte.js'
import { escapeHtml } from '#lib/tooltip/tooltip.js'
import type { TransferOperationType } from '#lib/file-explorer/types.js'
import type { ServerField, ServerProposalView } from '#lib/ipc/bindings.js'
import type { SuggestedGroupView } from '#lib/tauri-commands/index.js'
import { getUserFriendlyMessage } from '#lib/file-operations/transfer/transfer-error-messages.js'
import { wordConnectRefusal, type RefusalSubject } from '#lib/servers/connect-refusals.js'
import { readSavedServerOutcome } from '#lib/servers/server-outcomes.js'
import type { GroupRefusal, ServerRefusal } from './suggested-ops-trigger.svelte'

/** What sits under the group. `message` is HTML (the pipeline escapes every name and path), so
 *  it renders through `{@html}`, the boundary the queue's failed row uses too. `suggestion` is
 *  `null` where the message already says what to do (the servers sheet's one-sentence refusals). */
export interface RefusalReason {
  message: string
  suggestion: string | null
}

/**
 * Which `errors.write.<field>.<op>` arm a group's verb reads from. A `Record` over every verb
 * makes a new verb a compile error here rather than a missing key at runtime. A rename runs as
 * a move where it can't rename in place, and extract is a copy out of an archive. A server
 * suggestion runs no transfer, so it has no arm: the write engine never refuses one.
 */
type FileVerb = Exclude<SuggestedGroupView['verb'], 'addServer' | 'editServer'>
const OPERATION_TYPE: Record<FileVerb, TransferOperationType> = {
  move: 'move',
  copy: 'copy',
  extract: 'copy',
  compress: 'compress',
  trash: 'trash',
  delete: 'delete',
  rename: 'move',
}

/** `server` is the card a server group's refusal names its host and account from. */
export function refusalReason(
  refusal: GroupRefusal,
  verb: SuggestedGroupView['verb'],
  server: ServerProposalView | null = null,
): RefusalReason {
  switch (refusal.kind) {
    case 'refused': {
      // Unreachable for a server suggestion (the backend answers `couldNotStart`), so it reads that way.
      if (verb === 'addServer' || verb === 'editServer') return refusalReason({ kind: 'couldNotStart' }, verb)
      const friendly = getUserFriendlyMessage(refusal.error, OPERATION_TYPE[verb])
      return { message: friendly.message, suggestion: friendly.suggestion }
    }
    case 'server':
      return serverRefusalReason(refusal.answer, server)
    case 'nothingToRun':
      return {
        message: tString('suggestedOps.refusal.nothingToRun.message'),
        suggestion: tString('suggestedOps.refusal.nothingToRun.suggestion'),
      }
    case 'couldNotStart':
      return couldNotStart()
    case 'nothingSelected':
      return {
        message: tString('suggestedOps.refusal.nothingSelected.message'),
        suggestion: tString('suggestedOps.refusal.nothingSelected.suggestion'),
      }
  }
}

function couldNotStart(): RefusalReason {
  return {
    message: tString('suggestedOps.refusal.couldNotStart.message'),
    suggestion: tString('suggestedOps.refusal.couldNotStart.suggestion'),
  }
}

/**
 * A server card that came back unsaved. The save's own refusals read as the servers sheet says
 * them; the three only a suggestion meets (the server moved, it got saved, its name got taken,
 * all since the AI suggested it) have their own words.
 */
function serverRefusalReason(answer: ServerRefusal, server: ServerProposalView | null): RefusalReason {
  const subject = refusalSubject(server)
  switch (answer.kind) {
    case 'refused': {
      const outcome = readSavedServerOutcome(answer.outcome)
      // A `saved` outcome can't come back as a refusal; word it as the internal slip it'd be.
      if (outcome.kind === 'saved') return couldNotStart()
      return sheetSentence(
        wordConnectRefusal(outcome.refusal, { ...subject, takenBy: outcome.takenBy, share: outcome.share }),
      )
    }
    case 'addServer':
      // The add sheet's own reading: an address that doesn't parse is a typo, anything else unreachable.
      return sheetSentence(
        wordConnectRefusal(answer.error.type === 'invalid_address' ? 'invalid_url' : 'unreachable', subject),
      )
    case 'serverChanged':
      return {
        message: tString('suggestedOps.refusal.serverChanged.message'),
        suggestion: tString('suggestedOps.refusal.serverChanged.suggestion'),
      }
    case 'alreadySaved':
      return {
        message: tString('suggestedOps.refusal.alreadySaved.message'),
        suggestion: tString('suggestedOps.refusal.alreadySaved.suggestion'),
      }
    case 'nameTaken':
      return {
        message: tString('suggestedOps.refusal.nameTaken.message', { name: escapeHtml(answer.name) }),
        suggestion: tString('suggestedOps.refusal.nameTaken.suggestion'),
      }
  }
}

/** One of the servers sheet's sentences, which carry their own fix. Plain text, so escaped whole. */
function sheetSentence(sentence: string): RefusalReason {
  return { message: escapeHtml(sentence), suggestion: null }
}

/** The host and account a sheet sentence names, from the card's own rows. */
function refusalSubject(server: ServerProposalView | null): RefusalSubject {
  const valueOf = (field: ServerField) => server?.fields.find((row) => row.field === field)?.value ?? ''
  return { host: valueOf('address'), username: valueOf('account'), protocol: server?.protocol }
}
