/**
 * The Debug window's content, or `null` in a release build.
 *
 * The window is dev-only: its one opener, ⌘⇧D, fires only under `import.meta.env.DEV`
 * (`routes/(main)/global-keydown.ts`). SvelteKit still builds every route, though, so a static
 * page would ship all of it to users: every panel, the Components and Graphics catalogs, and the
 * dialog gallery's registry (about 130 kB of JS and 17 kB of CSS). Vite inlines `DEV` to a
 * build-time boolean, so in a release this branch folds to `null` and the dynamic import never
 * enters the module graph. Lives in a `.ts` file because a `.svelte` script can't name
 * `import.meta.env.DEV` without blinding knip (`src/DETAILS.md` § Dev gates).
 */
export function loadDebugWindow(): Promise<typeof import('./DebugPage.svelte')> | null {
  return import.meta.env.DEV ? import('./DebugPage.svelte') : null
}
