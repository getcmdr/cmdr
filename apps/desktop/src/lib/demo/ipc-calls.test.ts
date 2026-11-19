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

describe('the scanners', () => {
  it('find a raw invoke in each shape the app writes one', () => {
    const text = `
      await invoke('show_main_window', { reason })
      return invoke<PreviewPayload>('prepare_error_report_preview', { userNote })
      void invoke( "record_breadcrumb" , { event })
      send('plugin:process|exit', { code: 0 })
    `
    expect(findRawInvokes(text)).toEqual([
      'show_main_window',
      'prepare_error_report_preview',
      'record_breadcrumb',
      'plugin:process|exit',
    ])
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
})

describe("the demo's coverage of every way the app calls the backend", () => {
  it('scans the app (the glob found its sources)', () => {
    expect(files.length).toBeGreaterThan(500)
  })

  it('answers every raw invoke the app sends, or says why the demo leaves it out', () => {
    const gaps: string[] = []
    for (const [file, text] of files) {
      for (const command of findRawInvokes(text)) {
        if (!isWireAnswered(command) && whyNotInDemo(command) === undefined) gaps.push(`${command} (${file})`)
      }
    }
    // Answer it (`DETAILS.md` § Answering a command, step 6) or add it to `wireNotInDemo`.
    expect(gaps).toEqual([])
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
      ...files.flatMap(([, text]) => findRawInvokes(text)),
      ...Object.values(TAURI_API_CALLS).flat(),
    ])
    for (const [command, reason] of Object.entries(wireNotInDemo)) {
      expect(sent.has(command), `nothing in the app sends ${command}`).toBe(true)
      expect(isWireAnswered(command), `${command} is answered, so it isn't left out`).toBe(false)
      expect(reason.trim(), command).not.toBe('')
    }
  })
})
