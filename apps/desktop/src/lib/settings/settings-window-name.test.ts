/**
 * The settings window's title follows its menu item on every system: what the menu
 * row says is what the window says, and what every "open Settings" pointer says
 * (`docs/style-guide.md` § "Platform vocabulary").
 */
import { afterEach, describe, expect, it } from 'vitest'

import { tString } from '#lib/intl/messages.svelte.js'
import { COPY_PLATFORM_GLOBAL } from '#lib/platform/index.js'
import { withConventions, withPlatform } from '#lib/platform/test-platform.js'

/** The first word, without a trailing ellipsis: “Configure Cmdr…” → “Configure”. */
function firstWord(text: string): string {
  return text.replace(/…$/u, '').split(/[\s–]/u)[0]
}

const ROWS = [
  { name: 'macOS', apply: () => withPlatform('macos'), word: 'Settings' },
  { name: 'Windows', apply: () => withPlatform('windows'), word: 'Settings' },
  { name: 'GNOME', apply: () => withPlatform('linux', 'gnome'), word: 'Preferences' },
  { name: 'KDE Plasma', apply: () => withPlatform('linux', 'kde'), word: 'Configure' },
  { name: 'other Linux', apply: () => withPlatform('linux', 'other'), word: 'Settings' },
] as const

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

  for (const row of ROWS) {
    it(`reads “${row.word}” in the window title and the menu item on ${row.name}`, () => {
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
