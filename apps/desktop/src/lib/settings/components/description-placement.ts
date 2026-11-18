/**
 * Where a settings row shows its description, given the registry's placement. One rule for both
 * renderers: `SettingRow` and the Advanced page's auto-rendered rows. Every visible line sits under
 * the label, in the label column; the only other place is the ⓘ after the label.
 */

export interface DescriptionPlacementInput {
  placement: 'inline' | 'tip'
  /** The full description; `''` when the setting has none. */
  description: string
  /** A tip setting's short summary line; `''` when it has none. */
  summary: string
  /** Settings search's query, as typed. */
  searchQuery: string
}

export interface DescriptionPlacementView {
  /** The ⓘ's text after the label, or `''` for no ⓘ. */
  tip: string
  /** Lines under the label, in the label column, top to bottom. */
  underLabel: string[]
}

/**
 * An inline description is one line under the label. A tip's summary goes there too, and while
 * there's a query so does the tip's text, with the ⓘ stepping aside: search matches description
 * text, and a hit on words nobody can see would read as a wrong result.
 */
export function placeDescription({
  placement,
  description,
  summary,
  searchQuery,
}: DescriptionPlacementInput): DescriptionPlacementView {
  if (placement === 'inline') return { tip: '', underLabel: description === '' ? [] : [description] }
  const searching = searchQuery.trim() !== ''
  const underLabel = [summary, searching ? description : ''].filter((line) => line !== '')
  return { tip: searching ? '' : description, underLabel }
}
