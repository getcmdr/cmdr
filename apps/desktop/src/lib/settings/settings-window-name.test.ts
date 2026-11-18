/**
 * The settings window's title follows its menu item on every system: what the menu
 * row says is what the window says, and what every "open Settings" pointer says
 * (`docs/style-guide.md` § "Platform vocabulary").
 *
 * English only, on purpose: Vitest renders the `en` catalog. A translation follows
 * each desktop's own catalog instead, where the title and the menu row can share a
 * root but not a first word (KDE's Russian `Настройка – Cmdr` beside `Настроить Cmdr…`),
 * so no locale is held to this test (`docs/i18n/translator-instructions.md`, Per-OS values).
 */
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'

import { tString } from '#lib/intl/messages.svelte.js'
import { parsePlatformSelect } from '#lib/intl/platform-select.js'
import { COPY_PLATFORM_GLOBAL } from '#lib/platform/index.js'
import { withConventions, withPlatform } from '#lib/platform/test-platform.js'

/** The first word, without a trailing ellipsis: “Configure Cmdr…” → “Configure”. */
function firstWord(text: string): string {
  return text.replace(/…$/u, '').split(/[\s–]/u)[0]
}

const ROWS = [
  { name: 'macOS', apply: () => withPlatform('macos'), word: 'Settings', title: 'Settings', menu: 'Settings…' },
  { name: 'Windows', apply: () => withPlatform('windows'), word: 'Settings', title: 'Settings', menu: 'Settings' },
  {
    name: 'GNOME',
    apply: () => withPlatform('linux', 'gnome'),
    word: 'Preferences',
    title: 'Preferences',
    menu: 'Preferences',
  },
  {
    name: 'KDE Plasma',
    apply: () => withPlatform('linux', 'kde'),
    word: 'Configure',
    title: 'Configure – Cmdr',
    menu: 'Configure Cmdr…',
  },
  {
    name: 'other Linux',
    apply: () => withPlatform('linux', 'other'),
    word: 'Settings',
    title: 'Settings',
    menu: 'Settings…',
  },
] as const

/** Every `en` value that names the settings window per desktop: the pointers that follow the title. */
function settingsWindowPointers(): { key: string; value: string }[] {
  const dir = join(import.meta.dirname, '../intl/messages/en')
  return readdirSync(dir)
    .filter((file) => file.endsWith('.json'))
    .flatMap((file) => Object.entries(JSON.parse(readFileSync(join(dir, file), 'utf8')) as Record<string, unknown>))
    .flatMap(([key, value]) => {
      if (key.startsWith('@') || typeof value !== 'string') return []
      const branches = parsePlatformSelect(value)
      const names =
        /Preferences|preferences/.test(branches?.linuxGnome ?? '') || /Configure/.test(branches?.linuxKde ?? '')
      return names ? [{ key, value }] : []
    })
}

describe('the settings window name', () => {
  let undo: (() => void) | undefined
  afterEach(() => {
    undo?.()
    undo = undefined
  })

  it('follows a mocked desktop in dev, so CMDR_MOCK_CONVENTIONS=gnome on a Mac reads GNOME’s name', () => {
    const undoPlatform = withPlatform('macos')
    const undoConventions = withConventions('gnome')
    const host = globalThis as Record<string, unknown>
    host[COPY_PLATFORM_GLOBAL] = 'linux'
    undo = () => {
      Reflect.deleteProperty(host, COPY_PLATFORM_GLOBAL)
      undoConventions()
      undoPlatform()
    }
    expect(tString('settings.window.title')).toBe('Preferences')
  })

  // Each translation needs Windows' own branch here: Microsoft's word for an app's settings can differ from
  // Apple's (French Paramètres beside Réglages), even where the English is the same on both.
  it('gives Windows its own branch in every pointer to the window, so a translation can use Microsoft’s word', () => {
    const pointers = settingsWindowPointers()
    expect(pointers.length).toBeGreaterThan(50)
    const withoutWindows = pointers.filter(({ value }) => parsePlatformSelect(value)?.windows === undefined)
    expect(withoutWindows.map(({ key }) => key)).toEqual([])
  })

  for (const row of ROWS) {
    it(`reads “${row.title}” as the window title and “${row.menu}” as the menu item on ${row.name}`, () => {
      undo = row.apply()
      expect(tString('settings.window.title')).toBe(row.title)
      expect(tString('menu.app.settings')).toBe(row.menu)
    })

    it(`starts the window title and the menu item with “${row.word}” on ${row.name}`, () => {
      undo = row.apply()
      expect(firstWord(tString('settings.window.title'))).toBe(row.word)
      expect(firstWord(tString('menu.app.settings'))).toBe(row.word)
    })

    it(`points the user at “${row.word}” on ${row.name}`, () => {
      undo = row.apply()
      expect(tString('shortcuts.window.editInSettings')).toContain(row.word)
      expect(tString('onboarding.stepOptional.changeAnytime')).toContain(row.word)
      expect(tString('ai.translateError.off.body')).toContain(row.word)
    })
  }
})
