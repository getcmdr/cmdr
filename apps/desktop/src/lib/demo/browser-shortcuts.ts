/**
 * The demo's browser-safe shortcut maps, one arm per visitor OS: the keys a browser takes before
 * the page sees them come off, so no hint in the demo names a key that can't reach it. Two seeds
 * in `shortcuts.json`, so the app's shortcut system needs no demo awareness and every hint
 * (menus, palette, function bar, chips) shows the remapped combo:
 *
 * - {@link browserSafeShortcuts}: a rebindable command's, as `shortcut:<id>` user customizations,
 *   the same delta the Settings editor writes.
 * - {@link browserHostDefaults}: a fixed-key command's, as `default:<id>` host defaults
 *   (`lib/shortcuts/DETAILS.md` § Host defaults). The store drops a fixed-key command's
 *   customization, and its own matcher (`eventMatchesCommand`) reads the default.
 *
 * Arms:
 *
 * - macOS: every default the browser takes (new tab, close tab, reopen tab, new window, switch to
 *   tab N) moves to Control, which browsers on macOS leave alone. PLACEHOLDER: David decides the
 *   real one (issue #302).
 * - Windows and Linux: the registry's own defaults for that OS, minus the combos on
 *   {@link BROWSER_WITHHELD}. A command left with no key gets its {@link STAND_INS} entry where it
 *   has one (PLACEHOLDER, as the Mac arm's), else none.
 */
import type { CommandId } from '#lib/commands/command-ids.js'
import { commands, defaultShortcuts } from '#lib/commands/command-registry.js'
import type { Platform } from '#lib/platform/index.js'

/** A seeded list per command: the whole list, as the Settings editor stores one. */
export type BrowserSafeMap = Partial<Record<CommandId, string[]>>

const MAC_VISITOR_MAP: BrowserSafeMap = {
  'tab.new': ['⌃T'],
  'tab.close': ['⌃W'],
  'tab.reopen': ['⌃⇧T'],
  'askCmdr.newChat': ['⌃N'],
  'view.fullMode': ['⌃1'],
  'view.briefMode': ['⌃2'],
  'sort.byName': ['⌃3', '⌘F3'],
  'sort.byExtension': ['⌃4', '⌘F4'],
  'sort.byModified': ['⌃5', '⌘F5'],
  'sort.bySize': ['⌃6', '⌘F6'],
}

/** The search dialog's fixed keys on a Mac, off ⌘N (new window) and ⌘1–⌘3 (switch to tab N), as the map above moves them. */
const MAC_VISITOR_HOST_DEFAULTS: BrowserSafeMap = {
  'search.newQuery': ['⌃N'],
  'search.modeByNumber': ['⌃1', '⌃2', '⌃3'],
}

/**
 * The keys Chromium never hands a page on Windows and Linux, outside full screen: the accelerators
 * of its reserved commands (`kReservedCommandIds`: close tab and window, new tab, window, and
 * incognito window, reopen tab, next and previous tab). Verified in Chromium's source,
 * `chrome/browser/ui/accelerator_table.cc` and `browser_command_controller.cc`
 * (`IsReservedCommandOrKey`) at `c52b7d226b`, 2026-11-12. Every other browser key (`Alt+←` / `Alt+→`,
 * `F1`, `F5`, `F11`, `Ctrl+1`–`Ctrl+9`, `Ctrl+F`, …) still reaches the page, which may cancel it, so
 * those stay the demo's.
 */
const CHROMIUM_RESERVED_ON_WINDOWS_AND_LINUX = [
  'Ctrl+W',
  'Ctrl+F4',
  'Ctrl+Shift+W',
  'Alt+F4',
  'Ctrl+T',
  'Ctrl+N',
  'Ctrl+Shift+N',
  'Ctrl+Shift+T',
  'Ctrl+Tab',
  'Ctrl+Shift+Tab',
  'Ctrl+PageDown',
  'Ctrl+PageUp',
]

/**
 * Firefox's `reserved="true"` keys off macOS, which a page never sees: new window and tab, close
 * tab and window, new private window (`Ctrl+Shift+P`), and quit (`Ctrl+Shift+Q` on Windows,
 * `Ctrl+Q` elsewhere). Verified in `browser/base/content/browser-sets.inc` (`mainKeyset`),
 * mozilla/gecko-dev HEAD, 2026-11-19. Its other keys, `F1` (help) included, reach the page first.
 */
const FIREFOX_RESERVED = {
  windows: ['Ctrl+N', 'Ctrl+T', 'Ctrl+W', 'Ctrl+Shift+W', 'Ctrl+Shift+P', 'Ctrl+Shift+Q'],
  linux: ['Ctrl+N', 'Ctrl+T', 'Ctrl+W', 'Ctrl+Shift+W', 'Ctrl+Shift+P', 'Ctrl+Q'],
}

/**
 * Chromium's quit on Linux (`Ctrl+Shift+Q`, `IDC_EXIT`): `IsReservedCommandOrKey` names it as one of
 * the two commands that stay the browser's even in full screen, so it's reserved outside it too
 * (read off `browser_command_controller.cc`, chromium/chromium HEAD, 2026-11-19; not tried in a
 * browser). Windows has no Chromium quit key.
 */
const CHROMIUM_QUIT_ON_LINUX = ['Ctrl+Shift+Q']

/** The combos a browser keeps from the page, per visitor OS, in that OS's spelling: Chromium's and Firefox's together. */
export const BROWSER_WITHHELD: Readonly<Record<'windows' | 'linux', ReadonlySet<string>>> = {
  windows: new Set([...CHROMIUM_RESERVED_ON_WINDOWS_AND_LINUX, ...FIREFOX_RESERVED.windows]),
  linux: new Set([...CHROMIUM_RESERVED_ON_WINDOWS_AND_LINUX, ...FIREFOX_RESERVED.linux, ...CHROMIUM_QUIT_ON_LINUX]),
}

/**
 * A key for a command whose every default the browser keeps, where one fits: free in every
 * overlapping scope, and not one any browser on that OS keeps (`browser-shortcuts.test.ts` pins
 * both). PLACEHOLDER keys David decides, as the Mac arm's.
 *
 * - `search.newQuery` (Ctrl+N): `Alt+N`, beside the dialog's own Alt keys (`query-ui/query-keys.ts`).
 * - `app.commandPalette` (Ctrl+Shift+P, Firefox's private window): `F1`, VS Code's other palette key.
 * - `queue.show` (Ctrl+Shift+Q, Firefox's quit on Windows and Chromium's on Linux): `Ctrl+Shift+O`.
 */
export const STAND_INS = {
  windows: { 'search.newQuery': ['Alt+N'], 'app.commandPalette': ['F1'], 'queue.show': ['Ctrl+Shift+O'] },
  linux: { 'search.newQuery': ['Alt+N'], 'app.commandPalette': ['F1'], 'queue.show': ['Ctrl+Shift+O'] },
} as const satisfies Record<'windows' | 'linux', BrowserSafeMap>

/**
 * The registry's defaults on `platform`, minus what the browser keeps, for every command (`fixed`
 * picks the fixed-key ones or the rebindable ones) that holds one. A command left with none takes
 * its stand-in, or no key.
 */
function withoutWithheld(platform: 'windows' | 'linux', fixed: boolean): BrowserSafeMap {
  const withheld = BROWSER_WITHHELD[platform]
  const standIns: BrowserSafeMap = STAND_INS[platform]
  const map: BrowserSafeMap = {}
  for (const { id, fixedKey } of commands) {
    if ((fixedKey === true) !== fixed) continue
    const defaults = defaultShortcuts(id, platform)
    if (!defaults.some((combo) => withheld.has(combo))) continue
    const kept = defaults.filter((combo) => !withheld.has(combo))
    map[id] = kept.length > 0 ? kept : [...(standIns[id] ?? [])]
  }
  return map
}

const BROWSER_SAFE_MAPS: Readonly<Record<Platform, () => BrowserSafeMap>> = {
  macos: () => MAC_VISITOR_MAP,
  windows: () => withoutWithheld('windows', false),
  linux: () => withoutWithheld('linux', false),
}

const BROWSER_HOST_DEFAULTS: Readonly<Record<Platform, () => BrowserSafeMap>> = {
  macos: () => MAC_VISITOR_HOST_DEFAULTS,
  windows: () => withoutWithheld('windows', true),
  linux: () => withoutWithheld('linux', true),
}

/** The rebindable commands' map to seed as customizations for a visitor on `platform` (`./platform.ts`'s answer). */
export function browserSafeShortcuts(platform: Platform): BrowserSafeMap {
  return BROWSER_SAFE_MAPS[platform]()
}

/** The fixed-key commands' map to seed as host defaults for a visitor on `platform`. */
export function browserHostDefaults(platform: Platform): BrowserSafeMap {
  return BROWSER_HOST_DEFAULTS[platform]()
}
