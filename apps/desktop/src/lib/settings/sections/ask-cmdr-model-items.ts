/**
 * The "Ask Cmdr model" picker's menu (`AskCmdrSection.svelte`), as a pure function so the
 * ordering and the keep-what-the-user-typed rule are testable without Ark UI.
 *
 * The stored value is `askCmdr.interactiveModel`: `''` means "use the same model as Cmdr's AI"
 * (the backend reads it that way, `settings::load_ask_cmdr_interactive_model`). The menu can't
 * use `''` as an item value (Ark's select reads an empty value as "nothing picked"), so that
 * row carries `SAME_AS_CMDR_AI` and `storedModelFromPick` maps it back.
 */

/** One menu row; a structural subset of `ui/Select`'s `SelectItem` (a `.ts` file can't type-resolve a `.svelte` export for eslint). */
export interface ModelMenuItem {
  value: string
  label: string
}

/** The menu value standing for the stored `''`. Never a real model id (no provider names one this way). */
export const SAME_AS_CMDR_AI = '__same-as-cmdr-ai__'

export interface AskCmdrModelItemsInput {
  /** The stored override, trimmed: `''` follows Cmdr's AI. */
  current: string
  /** The provider's model list (from the session model cache or a connection check). May be empty while it loads. */
  models: readonly string[]
  /** The first row's label, already worded (it names the shared model when one is known). */
  sameAsLabel: string
}

/**
 * The first row is always "Same as Cmdr's AI". Then a stored override the list doesn't have (typed
 * by hand before this was a dropdown, or a model the provider dropped), so the user still sees, and
 * keeps, what they set. Then the provider's models, deduplicated, in the provider's order.
 */
export function askCmdrModelItems({ current, models, sameAsLabel }: AskCmdrModelItemsInput): ModelMenuItem[] {
  const items: ModelMenuItem[] = [{ value: SAME_AS_CMDR_AI, label: sameAsLabel }]
  const seen = new Set<string>()
  if (current !== '' && !models.includes(current)) {
    items.push({ value: current, label: current })
    seen.add(current)
  }
  for (const model of models) {
    if (model === '' || seen.has(model)) continue
    seen.add(model)
    items.push({ value: model, label: model })
  }
  return items
}

/** The menu value for a stored override. */
export function pickFromStoredModel(stored: string): string {
  return stored === '' ? SAME_AS_CMDR_AI : stored
}

/** The value to store for a menu pick. */
export function storedModelFromPick(pick: string): string {
  return pick === SAME_AS_CMDR_AI ? '' : pick
}
