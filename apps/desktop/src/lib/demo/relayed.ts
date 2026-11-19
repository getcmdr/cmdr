/**
 * Answers that tell the windows something changed, as the Rust side emits after the command:
 * the favorites edits (`volumes-changed`, the list the volume picker and the favorites menu read)
 * and a restricted window's setting write (`persist-restricted-setting`, which the main window
 * persists, as the desktop's `persist_restricted_window_setting` forwards it). The backend wires
 * the emit (`backend.ts`).
 */
import type { AnswerTable, CommandName } from './answer-types.js'
import type { EventTarget } from './backend.js'
import { FAVORITE_COMMANDS, favoriteAnswers } from './favorites.js'
import type { RestrictedWindowPersistableSetting } from '#lib/ipc/bindings.js'
import { demoLocations } from './volumes.js'

type Emit = (event: string, payload: unknown, target: EventTarget) => void

/** The setting id each restricted-window setting writes, as the desktop's `setting_id` names it. */
const SETTING_IDS: Record<RestrictedWindowPersistableSetting, string> = {
  viewerWordWrap: 'viewer.wordWrap',
  fileViewerSuppressBinaryWarning: 'fileViewer.suppressBinaryWarning',
}

/** The commands `relayedAnswers` answers, for the demo's coverage. */
export const RELAYED_COMMANDS = [
  ...FAVORITE_COMMANDS,
  'persistRestrictedWindowSetting',
] as const satisfies readonly CommandName[]

export function relayedAnswers(emit: Emit): AnswerTable {
  return {
    ...favoriteAnswers(() => {
      emit('volumes-changed', { data: demoLocations(), timedOut: false, discoveryPending: false }, { kind: 'everyone' })
    }),
    persistRestrictedWindowSetting: (p) => {
      const id = SETTING_IDS[p.setting as RestrictedWindowPersistableSetting]
      emit('persist-restricted-setting', { id, value: p.value === true }, { kind: 'label', label: 'main' })
      return null
    },
  }
}
