/**
 * Reads, from an app source's text, the backend calls the bindings don't type: raw `invoke`
 * commands and the Tauri JS APIs it imports. `ipc-calls.test.ts` runs these over every app source,
 * so a call the demo can't answer fails the change that adds it, as `coverage.ts` does for the
 * bindings. See `DETAILS.md` § Answering a command.
 */

/**
 * The wire names a source sends through a raw `invoke('…')` (generic type argument or not), plus
 * `app.html`'s boot guard, which calls the internals through its own `send('…')`.
 */
export function findRawInvokes(text: string): string[] {
  const found: string[] = []
  for (const match of text.matchAll(/(?<![.\w$])(?:invoke|send)(?:<[^()]*?>)?\(\s*['"`]([^'"`]+)['"`]/g)) {
    const command = match[1]
    // `send('…')` only counts when it names a command: a wire name is snake_case or `plugin:x|y`.
    if (/^(?:plugin:[a-z-]+\|)?[a-z][a-z0-9_]*$/.test(command)) found.push(command)
  }
  return found
}

/** A Tauri package's import path, short: `@tauri-apps/api/event` → `api/event`. */
function shortModule(specifier: string): string {
  return specifier.replace(/^@tauri-apps\//, '')
}

/** The value names in an import or destructuring list, by their exported name (`open as pick` → `open`). */
function valueNames(list: string): string[] {
  return list
    .split(',')
    .map((part) => part.trim())
    .filter((part) => part !== '' && !part.startsWith('type '))
    .map((part) => part.split(/\s+as\s+|\s*:\s*/)[0].trim())
}

/**
 * Every value a source imports from a `@tauri-apps/*` package, as `module#name`
 * (`api/event#listen`), from a static import or a destructured `await import(…)`. `unparsed` holds
 * any other dynamic import of one, whose names can't be read off the text.
 */
export function findTauriApiImports(text: string): { names: string[]; unparsed: string[] } {
  const names: string[] = []
  for (const match of text.matchAll(/\bimport\s*\{([^}]*)\}\s*from\s*['"](@tauri-apps\/[^'"]+)['"]/g)) {
    for (const name of valueNames(match[1])) names.push(`${shortModule(match[2])}#${name}`)
  }
  const destructured = /\b(?:const|let|var)\s*\{([^{}]*)\}\s*=\s*await\s+import\(\s*['"](@tauri-apps\/[^'"]+)['"]\s*\)/g
  const named = new Set<number>()
  for (const match of text.matchAll(destructured)) {
    for (const name of valueNames(match[1])) names.push(`${shortModule(match[2])}#${name}`)
    named.add((match.index ?? 0) + match[0].indexOf('import('))
  }
  const unparsed: string[] = []
  for (const match of text.matchAll(/\bimport\(\s*['"](@tauri-apps\/[^'"]+)['"]\s*\)/g)) {
    // A type position (`import('@tauri-apps/api/window').Window`) sends nothing.
    const after = text.slice((match.index ?? 0) + match[0].length)
    if (named.has(match.index ?? 0) || after.startsWith('.')) continue
    unparsed.push(`import('${match[1]}')`)
  }
  return { names, unparsed }
}
