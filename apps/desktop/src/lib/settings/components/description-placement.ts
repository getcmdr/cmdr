/**
 * Where a settings row shows its description, given the registry's placement. One rule for both
 * renderers: `SettingRow` and the Advanced page's auto-rendered rows.
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
  /** Lines under the label, in the label column. */
  underLabel: string[]
  /** The line under the whole row, or `''` for none. */
  inline: string
}

/**
 * Search matches description text, so while there's a query a tip's text shows under the label
 * too, and the ⓘ steps aside: a hit on words nobody can see would read as a wrong result.
 */
export function placeDescription({
  placement,
  description,
  summary,
  searchQuery,
}: DescriptionPlacementInput): DescriptionPlacementView {
  if (placement === 'inline') return { tip: '', underLabel: [], inline: description }
  const searching = searchQuery.trim() !== ''
  const underLabel = [summary, searching ? description : ''].filter((line) => line !== '')
  return { tip: searching ? '' : description, underLabel, inline: '' }
}
