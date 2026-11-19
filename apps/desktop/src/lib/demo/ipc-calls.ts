/**
 * Reads, from an app source's text, the backend calls the bindings don't type: raw `invoke`
 * commands and the Tauri JS APIs it imports. `ipc-calls.test.ts` runs these over every app source,
 * so a call the demo can't answer fails the change that adds it, as `coverage.ts` does for the
 * bindings. A call whose command or names can't be read off the text lands in `unparsed`, so a new
 * shape fails too instead of slipping past. See `DETAILS.md` § Answering a command.
 */

/** A wire name: snake_case, or `plugin:<name>|<command>`. */
const WIRE_NAME = /^(?:plugin:[a-z-]+\|)?[a-z][a-z0-9_]*$/

/**
 * `text` without its comment lines (a line that starts with `//`, `*`, or `/*`), so a doc comment
 * quoting a call isn't read as one. Line-start only: a `/*` inside a string (a glob) stays code.
 */
function withoutCommentLines(text: string): string {
  return text
    .split('\n')
    .map((line) => (/^\s*(?:\/\/|\/?\*)/.test(line) ? '' : line))
    .join('\n')
}

/** What one scan of a source finds: the readable names, and every call shape it can't read. */
export interface Found {
  names: string[]
  unparsed: string[]
}

/**
 * The wire names a source sends through a raw `invoke('…')`: a bare `invoke`, a member one
 * (`core.invoke('…')`, `window.__TAURI_INTERNALS__.invoke('…')`), with a generic type argument or
 * not. Plus `app.html`'s boot guard, which calls the internals through its own `send('…')` (a bare
 * `send` only, and only when it names a command). An `invoke` whose command isn't a literal
 * (`invoke(command)`, a template with `${…}`) is `unparsed`.
 */
export function findRawInvokes(text: string): Found {
  const code = withoutCommentLines(text)
  const names: string[] = []
  const unparsed: string[] = []
  for (const match of code.matchAll(/(?<![\w$])(?<!function\s+)invoke(?:<[^()]*?>)?\(/g)) {
    const after = code.slice(match.index + match[0].length)
    const literal = /^\s*(?:'([^'\n]*)'|"([^"\n]*)"|`([^`$]*)`)/.exec(after)
    // Whichever quote matched; the other groups are `undefined` at runtime, whatever their type says.
    const command = literal
      ? (literal.slice(1) as (string | undefined)[]).find((group) => group !== undefined)
      : undefined
    if (command !== undefined && WIRE_NAME.test(command)) names.push(command)
    else unparsed.push(`invoke(${after.slice(0, 40).split('\n')[0].trim()}`)
  }
  for (const match of code.matchAll(/(?<![.\w$])send\(\s*['"`]([^'"`]+)['"`]/g)) {
    if (WIRE_NAME.test(match[1])) names.push(match[1])
  }
  return { names, unparsed }
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
 * (`api/event#listen`): from a static import, a destructured `await import(…)`, or an
 * `import(…).then(({ … }) => …)`. `unparsed` holds any other shape that loads one, whose names
 * can't be read off the text: a namespace import, a dynamic import kept whole, a `.then` whose
 * callback takes the module whole.
 */
export function findTauriApiImports(text: string): Found {
  const code = withoutCommentLines(text)
  const names: string[] = []
  const unparsed: string[] = []
  for (const match of code.matchAll(/\bimport\s*\{([^}]*)\}\s*from\s*['"](@tauri-apps\/[^'"]+)['"]/g)) {
    for (const name of valueNames(match[1])) names.push(`${shortModule(match[2])}#${name}`)
  }
  for (const match of code.matchAll(/\bimport\s*\*\s*as\s+[\w$]+\s+from\s*['"](@tauri-apps\/[^'"]+)['"]/g)) {
    unparsed.push(`import * from '${match[1]}'`)
  }
  const destructured = /\b(?:const|let|var)\s*\{([^{}]*)\}\s*=\s*await\s+import\(\s*['"](@tauri-apps\/[^'"]+)['"]\s*\)/g
  const named = new Set<number>()
  for (const match of code.matchAll(destructured)) {
    for (const name of valueNames(match[1])) names.push(`${shortModule(match[2])}#${name}`)
    named.add(match.index + match[0].indexOf('import('))
  }
  for (const match of code.matchAll(/\bimport\(\s*['"](@tauri-apps\/[^'"]+)['"]\s*\)/g)) {
    if (named.has(match.index)) continue
    const after = code.slice(match.index + match[0].length)
    const then = /^\s*\.\s*then\(\s*(?:async\s*)?\(?\s*(\{[^{}]*\})?/.exec(after)
    if (then) {
      // `.then(({ emit }) => …)` names what it takes; `.then((mod) => …)` takes the module whole.
      const destructuring = then[1] as string | undefined
      if (destructuring === undefined) unparsed.push(`import('${match[1]}').then(…)`)
      else for (const name of valueNames(destructuring.slice(1, -1))) names.push(`${shortModule(match[1])}#${name}`)
      continue
    }
    // A type position (`import('@tauri-apps/api/window').Window`) sends nothing.
    if (/^\s*\.\s*[A-Z]/.test(after)) continue
    unparsed.push(`import('${match[1]}')`)
  }
  return { names, unparsed }
}
