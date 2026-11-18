/**
 * Validates the name typed into the New folder and New file dialogs.
 *
 * Sync checks first (the folder's name rules, name length, full path length),
 * then an async clash lookup against the live listing. The lookup re-runs on a
 * debounce while the user types and whenever a `directory-diff` lands for the
 * listing, so a folder created underneath the dialog shows up as a clash. The
 * dialog reads `errorMessage` / `isChecking`, and writes `errorMessage` back
 * when the create itself comes back refused.
 */

import { findFileIndex, getFileAt, getNameRules, onDirectoryDiff, type UnlistenFn } from '#lib/tauri-commands/index.js'
import { validateName, validatePathLength } from '#lib/utils/filename-validation.js'
import type { NameRules } from '#lib/ipc/bindings.js'
import { tString } from '#lib/intl/messages.svelte.js'
import type { ReportIncident } from '#lib/error-reporter/report-incident.js'
import type { CreateRefusal } from './create-submission.svelte'

/** Where the entry would land, and how to read the name being typed. */
export interface NewEntryNameCheckOptions {
  currentPath: string
  /** The folder's volume, for its name rules; absent reads as the local disk. */
  volumeId?: string
  listingId: string
  showHiddenFiles: boolean
  /** Read when the debounce fires, so a re-validation sees the latest keystroke. */
  getName: () => string
}

/** Long enough to coalesce keystrokes and for the listing cache to settle after a diff. */
const DEBOUNCE_MS = 100

export class NewEntryNameCheck {
  /** Empty while the name is acceptable (or still empty). */
  errorMessage = $state('')
  /** True while the clash lookup is in flight; the dialog holds OK until it lands. */
  isChecking = $state(false)
  /**
   * What "Send error report…" under the message reports, while the message is the
   * volume's refusal of a create. Any re-validation replaces the message, and this
   * goes with it, so the offer never sits under a message it isn't about.
   */
  refusal = $state<ReportIncident | null>(null)

  readonly #options: NewEntryNameCheckOptions
  /**
   * Which names the folder takes (`getNameRules`). `posix` until `listen` hears back; the
   * create itself checks the folder's own rules either way.
   */
  #rules: NameRules = 'posix'
  #timer: ReturnType<typeof setTimeout> | undefined
  /** Bumped per validate, so only the latest lookup's answer is shown. */
  #asked = 0
  #unlistenDiff: UnlistenFn | undefined

  constructor(options: NewEntryNameCheckOptions) {
    this.#options = options
  }

  /** The create itself came back refused: say why under the name, and offer the report. */
  showRefusal(refusal: CreateRefusal): void {
    this.errorMessage = refusal.message
    this.refusal = refusal.incident
  }

  async validate(name: string): Promise<void> {
    // ❗ Only the latest answer counts: a validate on open and one from a directory
    // diff overlap, and the older landing last flashed "already exists" for a free name.
    const asked = ++this.#asked
    this.refusal = null
    // A sync answer below supersedes any lookup still in flight, so it ends the wait too.
    this.isChecking = false
    const trimmed = name.trim()
    if (trimmed === '') {
      this.errorMessage = ''
      return
    }

    // Sync validators: the folder's name rules (characters, reserved names, length), full path length
    const nameCheck = validateName(trimmed, true, this.#rules)
    if (nameCheck.severity === 'error') {
      this.errorMessage = nameCheck.message
      return
    }
    const pathLenCheck = validatePathLength(this.#options.currentPath, trimmed)
    if (pathLenCheck.severity === 'error') {
      this.errorMessage = pathLenCheck.message
      return
    }

    // Sync checks passed: clear any previous error, then run the async clash check
    this.errorMessage = ''

    this.isChecking = true
    let message = ''
    try {
      const { listingId, showHiddenFiles } = this.#options
      const index = await findFileIndex(listingId, trimmed, showHiddenFiles)
      if (index !== null) {
        const entry = await getFileAt(listingId, index, showHiddenFiles)
        message = entry?.isDirectory
          ? tString('fileOperations.shared.conflictExistsFolder')
          : tString('fileOperations.shared.conflictExistsFile')
      }
    } catch {
      // If the lookup fails (listing gone), clear the error and let the backend decide
    }
    if (asked !== this.#asked) return
    this.errorMessage = message
    this.isChecking = false
  }

  /** Debounced `validate` of whatever the field holds when the timer fires. */
  schedule(): void {
    if (this.#timer) clearTimeout(this.#timer)
    this.#timer = setTimeout(() => {
      void this.validate(this.#options.getName())
    }, DEBOUNCE_MS)
  }

  /** Re-validates when the listing changes underneath the dialog, and once the folder's name rules land. */
  async listen(): Promise<void> {
    void this.#askNameRules()
    this.#unlistenDiff = await onDirectoryDiff((payload) => {
      if (payload.listingId !== this.#options.listingId) return
      this.schedule()
    })
  }

  async #askNameRules(): Promise<void> {
    try {
      const rules = await getNameRules(this.#options.volumeId ?? null, this.#options.currentPath)
      if (rules === this.#rules) return
      this.#rules = rules
      if (this.#options.getName().trim() !== '') this.schedule()
    } catch {
      // Keep `posix`: the create answers for the folder's own rules.
    }
  }

  dispose(): void {
    if (this.#timer) clearTimeout(this.#timer)
    this.#unlistenDiff?.()
  }
}
