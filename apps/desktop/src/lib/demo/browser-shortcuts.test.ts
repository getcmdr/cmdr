/**
 * The demo's browser-safe shortcut maps (`browser-shortcuts.ts`) have to hold up on every visitor
 * OS: every id a command, every combo one a keypress produces there, no new clash, and none of the
 * combos the browser takes before the page sees them, fixed-key commands included (those through
 * the store's host defaults). Off macOS the arm is the registry's defaults minus the browser's,
 * with a stand-in only where a command would be left with no key at all.
 */
import { beforeAll, describe, expect, it } from 'vitest'
import { COMMAND_IDS } from '#lib/commands/command-ids.js'
import { commands, defaultShortcuts } from '#lib/commands/command-registry.js'
import { PLATFORMS } from '#lib/commands/platform-shortcuts.js'
import type { Platform } from '#lib/platform/index.js'
import { withPlatform } from '#lib/platform/test-platform.js'
import { formatKeyCombo } from '#lib/shortcuts/key-capture.js'
import { scopesOverlap } from '#lib/shortcuts/scope-hierarchy.js'
import { BROWSER_WITHHELD, browserHostDefaults, browserSafeShortcuts, STAND_INS } from './browser-shortcuts.js'

/** The combos a browser keeps for itself on macOS: new tab or window, close tab, reopen tab, switch to tab N. */
const MAC_BROWSER_RESERVED = new Set([
  '⌘T',
  '⌘W',
  '⌘N',
  '⌘⇧T',
  ...Array.from({ length: 9 }, (_, i) => `⌘${String(i + 1)}`),
])

function withheldOn(platform: Platform): ReadonlySet<string> {
  return platform === 'macos' ? MAC_BROWSER_RESERVED : BROWSER_WITHHELD[platform]
}

type Flag = 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'

/** Each OS's modifier tokens as the registry spells them, with the flag each sets. */
const TOKENS: Record<Platform, readonly [token: string, flag: Flag][]> = {
  macos: [
    ['⌘', 'metaKey'],
    ['⌃', 'ctrlKey'],
    ['⌥', 'altKey'],
    ['⇧', 'shiftKey'],
  ],
  windows: [
    ['Ctrl+', 'ctrlKey'],
    ['Alt+', 'altKey'],
    ['Shift+', 'shiftKey'],
    ['Super+', 'metaKey'],
  ],
  linux: [
    ['Ctrl+', 'ctrlKey'],
    ['Alt+', 'altKey'],
    ['Shift+', 'shiftKey'],
    ['Super+', 'metaKey'],
  ],
}

const EVENT_KEYS: Record<string, string> = { '↑': 'ArrowUp', '↓': 'ArrowDown', '←': 'ArrowLeft', '→': 'ArrowRight' }

/** The keydown a stored combo stands for on `platform`. */
function keydownFor(combo: string, platform: Platform): KeyboardEvent {
  const event = { metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, key: '', code: '' }
  let rest = combo
  for (;;) {
    const match = TOKENS[platform].find(([token]) => rest.startsWith(token) && rest.length > token.length)
    if (match === undefined) break
    event[match[1]] = true
    rest = rest.slice(match[0].length)
  }
  event.key = EVENT_KEYS[rest] ?? (rest.length === 1 ? rest.toLowerCase() : rest)
  return event as unknown as KeyboardEvent
}

const isFixed = (id: string) => commands.find((c) => c.id === id)?.fixedKey === true

describe.each(PLATFORMS)('the browser-safe maps for a visitor on %s', (platform) => {
  beforeAll(() => withPlatform(platform))

  /** Both seeds: the customizations and the fixed-key commands' host defaults. */
  const seeds = (): [string, string[]][] => [
    ...Object.entries<string[]>(browserSafeShortcuts(platform)),
    ...Object.entries<string[]>(browserHostDefaults(platform)),
  ]

  /** Every command's combos as the demo seeds them: a seed's where it remaps, the registry's otherwise. */
  function effective(): { id: string; scope: string; shortcuts: readonly string[] }[] {
    const seeded = new Map(seeds())
    return commands.map((command) => ({
      id: command.id,
      scope: command.scope,
      shortcuts: seeded.get(command.id) ?? defaultShortcuts(command.id, platform),
    }))
  }

  it('remaps only real commands', () => {
    const known = new Set<string>(COMMAND_IDS)
    expect(
      seeds()
        .map(([id]) => id)
        .filter((id) => !known.has(id)),
    ).toEqual([])
  })

  it('spells every combo as a real keypress there produces it', () => {
    const misspelled = seeds().flatMap(([id, combos]) =>
      combos.filter((combo) => formatKeyCombo(keydownFor(combo, platform)) !== combo).map((c) => `${id}: ${c}`),
    )
    expect(misspelled).toEqual([])
  })

  it('leaves no command on a combo the browser keeps, fixed-key ones included', () => {
    const withheld = withheldOn(platform)
    const left = effective().flatMap(({ id, shortcuts }) =>
      shortcuts.filter((c) => withheld.has(c)).map((c) => `${id}: ${c}`),
    )
    expect(left).toEqual([])
  })

  it('clashes with no other command in an overlapping scope', () => {
    const all = effective()
    const clashes: string[] = []
    for (const [id, combos] of seeds()) {
      const own = all.find((c) => c.id === id)
      if (!own) continue
      for (const combo of combos) {
        for (const other of all) {
          if (other.id !== id && other.shortcuts.includes(combo) && scopesOverlap(own.scope, other.scope)) {
            clashes.push(`${combo}: ${id} vs ${other.id}`)
          }
        }
      }
    }
    expect(clashes).toEqual([])
  })

  // The store drops a fixed-key command's customization on load (`seedCustomShortcut`), so it has to
  // go in as a host default; a rebindable command's stays a customization, the delta Settings shows.
  it('seeds a fixed-key command only as a host default, and a rebindable one only as a customization', () => {
    expect(Object.keys(browserSafeShortcuts(platform)).filter(isFixed)).toEqual([])
    expect(Object.keys(browserHostDefaults(platform)).filter((id) => !isFixed(id))).toEqual([])
  })
})

describe.each(['windows', 'linux'] as const)('the %s arm', (platform) => {
  beforeAll(() => withPlatform(platform))
  const withheld = BROWSER_WITHHELD[platform]

  /** What the demo gives a command whose registry defaults on `platform` hold a withheld combo. */
  function expected(id: string): string[] {
    const kept = defaultShortcuts(id, platform).filter((c) => !withheld.has(c))
    const standIns: Partial<Record<string, readonly string[]>> = STAND_INS[platform]
    return kept.length > 0 ? kept : [...(standIns[id] ?? [])]
  }

  it('changes only commands holding a combo the browser keeps: drops it, or stands in where none is left', () => {
    const both = { ...browserSafeShortcuts(platform), ...browserHostDefaults(platform) }
    for (const [id, combos] of Object.entries<string[]>(both)) {
      expect(
        defaultShortcuts(id, platform).some((c) => withheld.has(c)),
        id,
      ).toBe(true)
      expect(combos, id).toEqual(expected(id))
    }
  })

  it('remaps every command that holds such a combo, fixed-key ones too', () => {
    const seeded = new Set([
      ...Object.keys(browserSafeShortcuts(platform)),
      ...Object.keys(browserHostDefaults(platform)),
    ])
    const missed = commands
      .filter((c) => defaultShortcuts(c.id, platform).some((combo) => withheld.has(combo)))
      .filter((c) => !seeded.has(c.id))
      .map((c) => c.id)
    expect(missed).toEqual([])
  })

  it('names every stand-in it uses, and uses every stand-in it names', () => {
    const named = Object.keys(STAND_INS[platform]).sort()
    const used = commands
      .map((c) => c.id)
      .filter((id) => defaultShortcuts(id, platform).length > 0)
      .filter((id) => defaultShortcuts(id, platform).every((c) => withheld.has(c)))
      .filter((id) => id in STAND_INS[platform])
      .sort()
    expect(used).toEqual(named)
  })

  // A decision, not a gap: the browser keeps every key these have, and no stand-in fits
  // (`Alt+T` / `Alt+W` are the in-page bar's mnemonics, `Ctrl+Alt+T` opens a terminal on Linux and
  // types a character under AltGr on some layouts). The Tab menu and the palette still reach them.
  it('leaves only the tab commands and Ask Cmdr’s new chat with no key', () => {
    const keyless = commands
      .filter((c) => defaultShortcuts(c.id, platform).length > 0)
      .filter((c) => expected(c.id).length === 0)
      .map((c) => c.id)
    expect(keyless).toEqual(
      platform === 'linux'
        ? ['tab.new', 'tab.close', 'tab.reopen', 'tab.next', 'tab.prev', 'askCmdr.newChat']
        : ['tab.new', 'tab.close', 'tab.reopen', 'askCmdr.newChat'],
    )
  })

  it("moves the search dialog's new query off Ctrl+N, and the command palette off Firefox's private window key", () => {
    expect(browserHostDefaults(platform)['search.newQuery']).toEqual(['Alt+N'])
    expect(browserSafeShortcuts(platform)['app.commandPalette']).toEqual(['F1'])
  })
})

describe('the macOS arm', () => {
  beforeAll(() => withPlatform('macos'))

  it("moves the search dialog's ⌘N and ⌘1–⌘3 to Control, as the rest of the arm does", () => {
    expect(browserHostDefaults('macos')).toEqual({
      'search.newQuery': ['⌃N'],
      'search.modeByNumber': ['⌃1', '⌃2', '⌃3'],
    })
  })
})
