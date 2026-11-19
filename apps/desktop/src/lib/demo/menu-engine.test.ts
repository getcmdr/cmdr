import { afterEach, describe, expect, it, vi } from 'vitest'

// The engine's glue only exists after `pnpm build:listing-wasm`, so the test stands a fake in for
// it with the same surface (`listing-wasm.d.ts`).
const engine = vi.hoisted(() => ({
  invoked: [] as [string, unknown][],
  platforms: [] as string[],
  failInit: false,
  menus: null as unknown[] | null,
}))

const MAC_BAR = [{ id: 'app', title: { text: 'Cmdr' }, rows: [{ kind: 'separator' }] }]

vi.mock('virtual:cmdr-listing-wasm', () => ({
  default: () => (engine.failInit ? Promise.reject(new Error('no wasm')) : Promise.resolve()),
  DemoMenuBar: class {
    constructor(platform: string) {
      engine.platforms.push(platform)
    }
    static handles(command: string): boolean {
      return command === 'activate_window_menu'
    }
    invoke(command: string, args: unknown): unknown {
      engine.invoked.push([command, args])
      return null
    }
    bar(): unknown {
      return engine.menus ?? MAC_BAR
    }
    click(menuId: string): unknown {
      return { kind: 'emit', event: 'execute-command', payload: { commandId: menuId } }
    }
  },
}))

import { _resetEngineModuleForTests } from './engine-module.js'
import { startMenuEngine } from './menu-engine.js'

afterEach(() => {
  _resetEngineModuleForTests()
  engine.invoked = []
  engine.platforms = []
  engine.failInit = false
  engine.menus = null
  vi.restoreAllMocks()
})

describe('menu engine', () => {
  it('routes the pushes the engine claims and answers the bar and a click from it', async () => {
    const handle = startMenuEngine('macos')
    expect(await handle.ready).toBe(true)
    expect(await handle.handles('activate_window_menu')).toBe(true)
    expect(await handle.handles('get_volume_space')).toBe(false)

    await handle.invoke('activate_window_menu', { label: 'main' })
    expect(engine.invoked).toEqual([['activate_window_menu', { label: 'main' }]])
    expect(handle.bar()).toEqual([{ id: 'app', title: { text: 'Cmdr' }, rows: [{ kind: 'separator' }] }])
    expect(handle.click('view.palette')).toEqual({
      kind: 'emit',
      event: 'execute-command',
      payload: { commandId: 'view.palette' },
    })
  })

  // Step 11 of task 63: the visitor's own OS's bar and menus, not the Mac's for everyone.
  it('builds the bar for the visitor’s own OS', async () => {
    const handle = startMenuEngine('windows')
    expect(await handle.ready).toBe(true)
    expect(engine.platforms).toEqual(['windows'])
  })

  // The Linux shortcut recorder warns about a combo the bar's Alt+letter takes: the letters are
  // the ones the drawn bar binds, by the app's own access-key rule over its titles.
  it('names the bar titles’ Alt letters off the Mac, and none on it', async () => {
    engine.menus = ['File', 'Edit', 'Find', 'View'].map((text) => ({ id: text, title: { text }, rows: [] }))
    const linux = startMenuEngine('linux')
    await linux.ready
    expect(await linux.titleMnemonics()).toEqual([
      { letter: 'f', menu: 'File' },
      { letter: 'e', menu: 'Edit' },
      { letter: 'i', menu: 'Find' },
      { letter: 'v', menu: 'View' },
    ])
    const mac = startMenuEngine('macos')
    expect(await mac.titleMnemonics()).toEqual([])
  })

  it('draws nothing, claims nothing, and refuses a push when the engine fails to load', async () => {
    engine.failInit = true
    vi.spyOn(console, 'error').mockImplementation(() => undefined)
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const handle = startMenuEngine('macos')
    expect(await handle.ready).toBe(false)
    expect(await handle.handles('activate_window_menu')).toBe(false)
    expect(handle.bar()).toBeNull()
    expect(handle.click('view.palette')).toEqual({ kind: 'nothing' })
    await expect(handle.invoke('activate_window_menu', {})).rejects.toThrow(/did not load/)
  })
})
