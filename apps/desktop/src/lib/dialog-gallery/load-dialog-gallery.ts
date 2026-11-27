/**
 * The one way the main window reaches the dialog gallery harness.
 *
 * Returns the harness module in dev and E2E builds, `null` in a release. Vite inlines both flags to
 * build-time booleans, so in a release the branch folds to `null` and the dynamic import never
 * enters the module graph: the harness, every fixture module, and every dialog only the harness
 * imports leave the bundle. ❌ A static import under an `{#if}` doesn't do this: the component
 * drops, but fixture modules whose top level calls a helper survive as side effects (about 45 kB
 * did, in the main layout's own chunk).
 *
 * The E2E flag is what lets the i18n screenshot driver and the E2E lane open gallery states on their
 * shared binary (`DETAILS.md`). Lives in a `.ts` file because a `.svelte` script can't name
 * `import.meta.env.DEV` without blinding knip (`src/DETAILS.md` § Dev gates).
 */
export function loadDialogGallery(): Promise<typeof import('./DialogGallery.svelte')> | null {
  return import.meta.env.DEV || __CMDR_E2E_BUILD__ ? import('./DialogGallery.svelte') : null
}
