/**
 * Which names the rename editor's folder takes (`getNameRules`), asked once per editor
 * session so every keystroke checks against the folder's filesystem, ❌ never the host OS's.
 *
 * `posix` until the answer lands: the backend's validity check on commit applies the
 * folder's own rules either way, so a late or failed answer only delays the hint.
 */

import { getNameRules } from '#lib/tauri-commands/index.js'
import type { NameRules } from '#lib/ipc/bindings.js'
import { POSIX_RULES, sameRules } from '#lib/utils/name-rules.js'
import type { createRenameState, RenameSessionId } from './rename-state.svelte'

export function createFolderNameRules(getVolumeId: () => string, rename: ReturnType<typeof createRenameState>) {
  let rules: NameRules = POSIX_RULES
  return {
    get current(): NameRules {
      return rules
    },
    /**
     * Starts over at `posix` for `sessionId`'s folder `dir`, and asks. An answer that
     * differs, landing while that session is still the open one, is taken, and `revalidate`
     * runs once the name was edited: an untouched name is one the folder already holds.
     */
    ask(dir: string, sessionId: RenameSessionId, revalidate: () => void): void {
      rules = POSIX_RULES
      getNameRules(getVolumeId(), dir).then(
        (answer) => {
          if (!rename.active || rename.isSuperseded(sessionId) || sameRules(answer, rules)) return
          rules = answer
          if (rename.hasChanged()) revalidate()
        },
        () => {},
      )
    },
  }
}
