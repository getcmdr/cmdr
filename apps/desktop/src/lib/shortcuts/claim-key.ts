/**
 * Claiming a keypress a local handler acted on.
 *
 * A keydown travels two roads at once: the element-level handler (a descendant of
 * `document`, so it runs first) and the document-level dispatcher in `+page.svelte`,
 * which looks the combo up in the Tier 1 map and runs the command it finds. The
 * dispatcher has no `defaultPrevented` guard, so `preventDefault()` alone does NOT
 * stop it — the command runs a second time, and when both ends land in the same
 * place the work simply happens twice with nothing looking wrong.
 *
 * `preventDefault` + `stopPropagation` together are the claim. This function is that
 * pair with a name, so the two halves can't drift apart: every instance of this bug
 * so far has been a branch that had the first call and not the second.
 *
 * ❗ Call it in every branch that ACTS on a key, bare keys included: `Enter`, `Tab`,
 * `Space`, `PageUp`/`PageDown`, `Home`/`End`, `F5` and `Insert` all belong to Tier 1
 * commands. ❌ Don't call it in a branch that decided NOT to act — that key belongs
 * to the dispatcher.
 *
 * What it has cost, each found by someone tripping over it: ⌘R in the former
 * Servers hub re-read every host's shares twice; Enter in the file pane opened a
 * file twice (two browser tabs for a Google Drive file); Enter in the former
 * Servers hub opened a host and mounted a share twice; PageDown moved two pages
 * instead of one.
 *
 * Enforced by `cmdr/claim-acted-key`.
 */
export function claimKey(event: Pick<KeyboardEvent, 'preventDefault' | 'stopPropagation'>): void {
  event.preventDefault()
  event.stopPropagation()
}
