/**
 * How a preview row's status reads in the list: problem rows get a glyph of their own
 * (a short label for its accessible name, the full reason for its tooltip), and ready
 * and unchanged rows stay quiet, with words for screen readers only. Pure: it names
 * message keys, and the sheet words them.
 */

import type { MessageKey } from '#lib/intl/keys.gen.js'
import type { MediaField, RowStatus } from '#lib/ipc/bindings.js'
import type { IconName } from '#lib/ui/icons/icon-map.js'
import { MEDIA_FIELD_LABELS } from './media-fields'
import { MEDIA_FIELDS } from './media-token'

export interface StatusMessage {
  key: MessageKey
  /** A param is text, or a message of its own (a media field's name), worded first. */
  params?: Record<string, string | { key: MessageKey }>
}

/** `message` in words: `words` is the catalog lookup (`tString`). */
export function statusWords(
  message: StatusMessage,
  words: (key: MessageKey, params?: Record<string, string>) => string,
): string {
  if (!message.params) return words(message.key)
  const params = Object.fromEntries(
    Object.entries(message.params).map(([name, value]) => [name, typeof value === 'string' ? value : words(value.key)]),
  )
  return words(message.key, params)
}

const fieldParams = (field: MediaField): StatusMessage['params'] => ({ field: { key: MEDIA_FIELD_LABELS[field] } })

export interface RowStatusView {
  /** Anything but ready or unchanged: the row won't be renamed, and says why. */
  isProblem: boolean
  /** The problem's glyph, one per kind; `null` for ready and unchanged rows. */
  glyph: IconName | null
  /** A couple of words: the glyph's accessible name, or a quiet row's screen-reader text. */
  label: StatusMessage
  /** The full reason, for the glyph's tooltip. `null` on a quiet row. */
  reason: StatusMessage | null
}

const INVALID_REASONS = {
  empty: 'multiRename.reason.empty',
  tooLong: 'multiRename.reason.tooLong',
  reserved: 'multiRename.reason.reserved',
  endsWithSpaceOrPeriod: 'multiRename.reason.endsWithSpaceOrPeriod',
} as const satisfies Record<string, MessageKey>

export function rowStatusView(status: RowStatus): RowStatusView {
  const problem = (glyph: IconName, label: StatusMessage, reason: StatusMessage): RowStatusView => ({
    isProblem: true,
    glyph,
    label,
    reason,
  })
  switch (status.type) {
    case 'ready':
      return { isProblem: false, glyph: null, label: { key: 'multiRename.status.ready' }, reason: null }
    case 'unchanged':
      return { isProblem: false, glyph: null, label: { key: 'multiRename.status.unchanged' }, reason: null }
    case 'duplicate':
      return problem('copy', { key: 'multiRename.status.duplicate' }, { key: 'multiRename.reason.duplicate' })
    case 'targetExists':
      return problem('circle-x', { key: 'multiRename.status.targetExists' }, { key: 'multiRename.reason.targetExists' })
    case 'missing':
      return problem('circle-dashed', { key: 'multiRename.status.missing' }, { key: 'multiRename.reason.missing' })
    // Still reading may yet turn ready: quiet, like ready and unchanged.
    case 'reading':
      return { isProblem: false, glyph: null, label: { key: 'multiRename.status.reading' }, reason: null }
    case 'noValue': {
      const params = fieldParams(status.field)
      // A date can fall back to the modified date: the reason says where.
      const reason =
        MEDIA_FIELDS[status.field].kind === 'date' ? 'multiRename.reason.noDate' : 'multiRename.reason.noValue'
      return problem('circle-alert', { key: 'multiRename.status.noValue', params }, { key: reason, params })
    }
    case 'unreadable':
      return problem(
        'triangle-alert',
        { key: 'multiRename.status.unreadable' },
        { key: 'multiRename.reason.unreadable', params: fieldParams(status.field) },
      )
    case 'invalidName': {
      const reason = status.reason
      if (reason.type === 'disallowedCharacter') {
        const params = { character: reason.character }
        return problem(
          'circle-slash',
          { key: 'multiRename.status.disallowedCharacter', params },
          { key: 'multiRename.reason.disallowedCharacter', params },
        )
      }
      if (reason.type === 'reservedDeviceName') {
        return problem(
          'circle-slash',
          { key: 'multiRename.status.reservedDeviceName' },
          { key: 'multiRename.reason.reservedDeviceName', params: { name: reason.name } },
        )
      }
      return problem('circle-slash', { key: 'multiRename.status.invalidName' }, { key: INVALID_REASONS[reason.type] })
    }
  }
}
