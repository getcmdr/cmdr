/**
 * Which names the transfer dialog's destination takes (`getNameRules`), asked again as the
 * typed destination settles. Feeds the leaf-name check (single item, rename mode) and the
 * upfront "won't work on Windows" warning (`transfer-name-portability.ts`).
 *
 * `POSIX_RULES` until an answer for the destination on screen lands, and whenever one doesn't
 * come (each answer carries the volume and path it answers, so a stale one can't apply): the
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

/** One answer, with the destination it answers for. */
interface Answer {
  volumeId: string
  path: string
  rules: NameRules
}

export function createTransferDestNameRules(deps: {
  getVolumeId: () => string
  getPath: () => string
  getDestroyed: () => boolean
}) {
  let answer = $state.raw<Answer | null>(null)
  let asked = 0
  // Only an answer for the destination on screen counts, so a failed lookup, an emptied
  // path, or a path still waiting for its own answer reads as POSIX, ❌ never as the
  // previous destination's rules.
  const live = $derived(
    answer !== null && answer.volumeId === deps.getVolumeId() && answer.path === deps.getPath() ? answer : null,
  )

  $effect(() => {
    const volumeId = deps.getVolumeId()
    const path = deps.getPath()
    const ask = ++asked
    if (path.trim() === '') return
    const timer = setTimeout(() => {
      getNameRules(volumeId, path).then(
        (rules) => {
          if (ask !== asked || deps.getDestroyed()) return
          if (
            answer === null ||
            answer.volumeId !== volumeId ||
            answer.path !== path ||
            !sameRules(rules, answer.rules)
          ) {
            answer = { volumeId, path, rules }
          }
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
      return live?.rules ?? POSIX_RULES
    },
    /**
     * The limits a typed path is held to: the destination's once it answered, else
     * `undefined`, which `validateDirectoryPath` reads as this OS's own.
     */
    get limits(): TypedPathLimits | undefined {
      return live === null ? undefined : limitsOf(live.rules)
    },
  }
}
