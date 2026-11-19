/**
 * The host's own defaults: keys a host that can't deliver some registry keys gives a command in
 * their place. Today that's only the browser demo, whose browser keeps `Ctrl+N` and `⌘N`; its
 * boot (`lib/demo/boot.ts`) calls {@link setHostDefaults} in every frame before any app module
 * runs. Nothing in the desktop app calls it, and no file feeds it, so a hand-edited
 * `shortcuts.json` can't rebind a fixed-key command. `shortcuts-store.ts` reads it through
 * `getDefaultShortcuts`. Its own module, free of imports, so the demo's boot can set it without
 * pulling in the store. See `DETAILS.md` § Host defaults.
 */

const hostDefaults = new Map<string, readonly string[]>()

/** Replaces the host defaults: per command, the whole list it takes in place of the registry's (`[]` for no key). */
export function setHostDefaults(map: Readonly<Partial<Record<string, readonly string[]>>>): void {
  hostDefaults.clear()
  for (const [commandId, combos] of Object.entries(map)) {
    if (combos !== undefined) hostDefaults.set(commandId, [...combos])
  }
}

/** The host's default for `commandId`, or `undefined` when the registry's stands. */
export function hostDefaultFor(commandId: string): readonly string[] | undefined {
  return hostDefaults.get(commandId)
}

/** Every command with a host default. */
export function hostDefaultIds(): string[] {
  return [...hostDefaults.keys()]
}
