import { describe, expect, it } from 'vitest'
import { findRawInvokes, findTauriApiImports } from './ipc-calls.js'
import { TAURI_API_CALLS, TAURI_API_HANDLES, isWireAnswered, whyNotInDemo, wireNotInDemo } from './coverage.js'

/**
 * Every app source a demo visitor's frames can run, as text. A runtime glob on purpose: it puts
 * every one of them in this test's module graph, so `svelte-tests-related` runs it on the change
 * that adds a raw `invoke` or a Tauri API call. Not the demo itself, the bindings (`coverage.test.ts`
 * covers those), tests, or the dev-only debug window (the demo refuses to open it).
 */
const sources = import.meta.glob<string>(
  [
    '/src/**/*.{ts,svelte}',
    '/src/app.html',
    '!/src/**/*.test.ts',
    '!/src/**/test-*.ts',
    '!/src/test-*.ts',
    '!/src/lib/demo/**',
    '!/src/lib/ipc/**',
    '!/src/routes/debug/**',
  ],
  { query: '?raw', import: 'default', eager: true },
)

const files = Object.entries(sources)

/**
 * The one `invoke` the scan can't read and may skip, per file: a forwarder whose own callers name
 * their commands with a literal, which the scan reads at THEIR call sites.
 */
const FORWARDERS: Readonly<Record<string, string>> = {
  // The boot guard's `send(command, args)`; its callers are read as `send('…')`.
  '/src/app.html': 'invoke(command, args)',
}

describe('the scanners', () => {
  it('find a raw invoke in each shape the app writes one', () => {
    const text = `
      await invoke('show_main_window', { reason })
      return invoke<PreviewPayload>('prepare_error_report_preview', { userNote })
      void invoke( "record_breadcrumb" , { event })
      send('plugin:process|exit', { code: 0 })
    `
    expect(findRawInvokes(text)).toEqual({
      names: ['show_main_window', 'prepare_error_report_preview', 'record_breadcrumb', 'plugin:process|exit'],
      unparsed: [],
    })
  })

  it('read a member invoke, as the internals or a namespace send one', () => {
    const text = `
      await core.invoke('read_file', { path })
      window.__TAURI_INTERNALS__.invoke<string>("plugin:fs|stat", {})
    `
    expect(findRawInvokes(text)).toEqual({ names: ['read_file', 'plugin:fs|stat'], unparsed: [] })
  })

  it('report an invoke whose command is not a literal, so it fails instead of slipping past', () => {
    const text = `
      await invoke(command, args)
      core.invoke(\`get_\${kind}\`)
    `
    expect(findRawInvokes(text).unparsed).toEqual(['invoke(command, args)', 'invoke(`get_${kind}`)'])
  })

  it('skip a call quoted in a comment line, a declaration, and a member send', () => {
    const text = `
      /** Calls \`invoke('quoted_in_docs')\` under the hood. */
      // invoke(somethingInAComment)
      function invoke(cmd: string) {}
      socket.send('not_a_command')
    `
    expect(findRawInvokes(text)).toEqual({ names: [], unparsed: [] })
  })

  it('find each value a module imports from a Tauri package, static or dynamic, and skip types', () => {
    const text = `
      import { type UnlistenFn, listen } from '@tauri-apps/api/event'
      import type { Store } from '@tauri-apps/plugin-store'
      import { open as openFolderPicker } from '@tauri-apps/plugin-dialog'
      const { writeFile } = await import('@tauri-apps/plugin-fs')
      const { getVersion, setTheme } = await import('@tauri-apps/api/app')
    `
    expect(findTauriApiImports(text)).toEqual({
      names: [
        'api/event#listen',
        'plugin-dialog#open',
        'plugin-fs#writeFile',
        'api/app#getVersion',
        'api/app#setTheme',
      ],
      unparsed: [],
    })
  })

  it('report a dynamic import it cannot name, so a new shape fails instead of slipping past', () => {
    expect(findTauriApiImports(`const mod = await import('@tauri-apps/api/window')`).unparsed).toEqual([
      "import('@tauri-apps/api/window')",
    ])
  })

  it('read the names an import().then callback destructures', () => {
    const text = `
      void import('@tauri-apps/api/event').then(({ emit }) => emit('x'))
      void import('@tauri-apps/plugin-fs').then(async ({ writeFile, mkdir: makeDir }) => {})
    `
    expect(findTauriApiImports(text)).toEqual({
      names: ['api/event#emit', 'plugin-fs#writeFile', 'plugin-fs#mkdir'],
      unparsed: [],
    })
  })

  it('report an import().then that takes the module whole, and a namespace import', () => {
    const text = `
      void import('@tauri-apps/plugin-fs').then((fs) => fs.writeFile(p, b))
      import('@tauri-apps/api/app').then(app => app.getVersion())
      import * as event from '@tauri-apps/api/event'
    `
    expect(findTauriApiImports(text).unparsed).toEqual([
      "import * from '@tauri-apps/api/event'",
      "import('@tauri-apps/plugin-fs').then(…)",
      "import('@tauri-apps/api/app').then(…)",
    ])
  })

  it('skip a dynamic import in a type position', () => {
    expect(findTauriApiImports(`let w: import('@tauri-apps/api/window').Window`)).toEqual({ names: [], unparsed: [] })
  })
})

describe("the demo's coverage of every way the app calls the backend", () => {
  it('scans the app (the glob found its sources)', () => {
    expect(files.length).toBeGreaterThan(500)
  })

  it('answers every raw invoke the app sends, or says why the demo leaves it out', () => {
    const gaps: string[] = []
    const unparsed: string[] = []
    for (const [file, text] of files) {
      const found = findRawInvokes(text)
      for (const command of found.names) {
        if (!isWireAnswered(command) && whyNotInDemo(command) === undefined) gaps.push(`${command} (${file})`)
      }
      for (const shape of found.unparsed) if (FORWARDERS[file] !== shape) unparsed.push(`${shape} (${file})`)
    }
    // Answer it (`DETAILS.md` § Answering a command, step 6) or add it to `wireNotInDemo`.
    expect(gaps).toEqual([])
    // Name the command with a literal, so the demo can be checked against it.
    expect(unparsed).toEqual([])
  })

  it('knows what each Tauri API the app imports sends', () => {
    const unknown: string[] = []
    const unparsed: string[] = []
    for (const [file, text] of files) {
      const found = findTauriApiImports(text)
      for (const name of found.names) {
        if (!(name in TAURI_API_CALLS) && !(name in TAURI_API_HANDLES)) unknown.push(`${name} (${file})`)
      }
      unparsed.push(...found.unparsed.map((shape) => `${shape} (${file})`))
    }
    // Add it to `TAURI_API_CALLS` with the commands it sends, or to `TAURI_API_HANDLES`.
    expect(unknown).toEqual([])
    expect(unparsed).toEqual([])
  })

  it('answers every command those APIs send, or says why the demo leaves it out', () => {
    const gaps = Object.entries(TAURI_API_CALLS).flatMap(([api, wires]) =>
      wires
        .filter((wire) => !isWireAnswered(wire) && whyNotInDemo(wire) === undefined)
        .map((wire) => `${wire} (${api})`),
    )
    expect(gaps).toEqual([])
  })

  it('names only APIs the app still imports', () => {
    const imported = new Set(files.flatMap(([, text]) => findTauriApiImports(text).names))
    const stale = [...Object.keys(TAURI_API_CALLS), ...Object.keys(TAURI_API_HANDLES)].filter(
      (api) => !imported.has(api),
    )
    expect(stale).toEqual([])
  })

  it('leaves out only wire commands the app sends and nothing answers, each with a reason', () => {
    const sent = new Set([
      ...files.flatMap(([, text]) => findRawInvokes(text).names),
      ...Object.values(TAURI_API_CALLS).flat(),
    ])
    for (const [command, reason] of Object.entries(wireNotInDemo)) {
      expect(sent.has(command), `nothing in the app sends ${command}`).toBe(true)
      expect(isWireAnswered(command), `${command} is answered, so it isn't left out`).toBe(false)
      expect(reason.trim(), command).not.toBe('')
    }
  })
})
