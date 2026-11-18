/**
 * Which names the transfer dialog's destination takes (`getNameRules`), asked again as the
 * typed destination settles. Feeds the leaf-name check (single item, rename mode) and the
 * upfront "won't work on Windows" warning (`transfer-name-portability.ts`).
 *
 * `POSIX_RULES` until the first answer lands, and whenever an answer doesn't come: the
 * transfer itself still meets the destination's own refusals, so a late or failed answer
 * only delays a hint. Debounced, latest-wins: a keystroke's answer never overwrites a
 * newer path's. Created synchronously in the dialog's init so its `$effect` is tracked.
 */

import { getNameRules } from '#lib/tauri-commands/index.js'
import type { NameRules } from '#lib/ipc/bindings.js'
import { limitsOf, type TypedPathLimits } from '#lib/utils/filename-validation.js'
import { POSIX_RULES, sameRules } from '#lib/utils/name-rules.js'

/** Long enough to coalesce typing; the backend's answer is one `statfs`. */
const DEBOUNCE_MS = 250

export function createTransferDestNameRules(deps: {
  getVolumeId: () => string
  getPath: () => string
  getDestroyed: () => boolean
}) {
  let rules = $state.raw<NameRules>(POSIX_RULES)
  let answered = $state(false)
  let asked = 0

  $effect(() => {
    const volumeId = deps.getVolumeId()
    const path = deps.getPath()
    const ask = ++asked
    if (path.trim() === '') return
    const timer = setTimeout(() => {
      getNameRules(volumeId, path).then(
        (answer) => {
          if (ask !== asked || deps.getDestroyed()) return
          answered = true
          if (!sameRules(answer, rules)) rules = answer
        },
        () => {},
      )
    }, DEBOUNCE_MS)
    return () => {
      clearTimeout(timer)
    }
  })

  return {
    get current(): NameRules {
      return rules
    },
    /**
     * The limits a typed path is held to: the destination's once it answered, else
     * `undefined`, which `validateDirectoryPath` reads as this OS's own.
     */
    get limits(): TypedPathLimits | undefined {
      return answered ? limitsOf(rules) : undefined
    },
  }
}
