/**
 * Keeps text the demo writes into its own DOM (a window button's `aria-label`) in the UI
 * language: `apply` reads its words through `getMessage`, which reads the locale-version rune, so
 * an effect re-runs it on every language switch. For chrome built outside any component.
 */

/** Runs `apply` now and again on every UI language change, until the returned stop runs. */
export function followLanguage(apply: () => void): () => void {
  apply()
  return $effect.root(() => {
    $effect(() => {
      apply()
    })
  })
}
