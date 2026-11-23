/**
 * The wrong-OS-word net over English platform blocks: no `windows` or Linux
 * (`linuxGnome`, `linuxKde`, `other`) branch of an `en` value names a macOS thing (Finder, Keychain, ⌘, a
 * `{system_settings}`-style token), and neither borrows the other's words
 * (Trash, Quit, or Terminal on Windows; Recycle Bin or "disconnect" on Linux).
 * The words are `docs/style-guide.md` § "Platform vocabulary". Reads the catalogs
 * from disk, so it guards any hand edit of an `en` block, the way a per-OS string
 * changes (`src/lib/intl/DETAILS.md` § Platform variants).
 *
 * A branch that keeps a macOS word on purpose goes in `EXCEPTIONS`, with the reason.
 *
 * A plain value shows the same text on every OS, so one that names a macOS thing
 * (`macWordsInPlainValue`: Finder, Mac, Full Disk Access, iCloud, Trash, Terminal, a
 * `{system_settings}`-style token, …) reads wrong on Windows and Linux unless only a
 * Mac ever shows it. Each such key, or a key prefix ending in `.`, sits in
 * `MAC_ONLY_PLAIN_VALUES` with the reason no other OS reaches it (usually the
 * capability that hides its surface); any other hit wants a `{platform, select}`
 * block. The rows still waiting on Windows code are `docs/notes/windows-port/copy-decisions.md`
 * § Open rows.
 *
 * Key glyphs are stricter: ⌘, ⌥, ⌃, and ⇧ also never appear in a plain value
 * (`macKeyGlyphsInPlainValue`). A key in a sentence is a `ShortcutChip` tag or a
 * `{combo}` param fed by `shortcutAsText`, which spell it per OS and follow a
 * rebind; a typed glyph shows a Mac key to Windows and Linux users and goes stale
 * the moment the user rebinds. A sentence that names the glyphs themselves keeps
 * them in a `macos` branch.
 *
 * A few words are one spelling on every OS ("filename", never "file name"; "KDE
 * Wallet", never "KWallet"), so
 * `everyOsWrongWords` reads every branch, macOS included, and the `@key` metadata.
 */

import { describe, expect, it } from 'vitest'
import { SYSTEM_TOKENS, loadCatalog, platformBranches, visibleText } from './i18n-catalog-lib.ts'

/** The macOS modifier glyphs. Off macOS the same keys are words (Ctrl, Alt, Shift, Win or Super). */
const MAC_KEY_GLYPH = /[⌘⌥⌃⇧]/u

/** One word rule: what it's called in a report, and what it matches in visible text. */
interface WordRule {
  rule: string
  pattern: RegExp
}

/** macOS words that never belong in a Windows or Linux branch. */
const MACOS_WORDS: readonly WordRule[] = [
  { rule: 'macOS word “Finder”', pattern: /\bFinder\b/ },
  { rule: 'macOS word “macOS”', pattern: /\bmacOS\b/ },
  { rule: 'macOS word “Mac”', pattern: /\bMacs?\b/ },
  { rule: 'macOS word “Quick Look”', pattern: /\bQuick Look\b/i },
  { rule: 'macOS word “Spotlight”', pattern: /\bSpotlight\b/ },
  { rule: 'macOS word “Keychain”', pattern: /\bKeychain\b/ },
  { rule: 'macOS word “Dock”', pattern: /\bDock\b/ },
  { rule: 'macOS key glyph (⌘, ⌥, ⌃, or ⇧)', pattern: MAC_KEY_GLYPH },
]

const WINDOWS_WRONG_WORDS: readonly WordRule[] = [
  { rule: '“Trash” on Windows: “Delete” in a label, “the Recycle Bin” in a sentence', pattern: /\btrash\b/i },
  { rule: '“Quit” on Windows: “Exit” for the action, “close” in narration', pattern: /\bquit(s|ting)?\b/i },
  { rule: '“Terminal” on Windows: “PowerShell”', pattern: /\bTerminal\b/ },
]

const LINUX_WRONG_WORDS: readonly WordRule[] = [
  { rule: '“Recycle Bin” on Linux: “Trash”', pattern: /\brecycle bin\b/i },
  { rule: '“disconnect” on Linux: a network mount is unmounted', pattern: /\bdisconnect\b/i },
]

/**
 * Words Cmdr spells one way on every OS, in every branch and every `@key` description
 * (style guide § "Platform vocabulary"). Apple writes "filename" and Microsoft "file
 * name"; Cmdr picks the one word everywhere so docs and UI agree.
 */
const EVERY_OS_WRONG_WORDS: readonly WordRule[] = [
  { rule: '“file name”: Cmdr writes “filename”, one word, on every OS', pattern: /\bfile names?\b/i },
  {
    rule: '“KWallet”: KDE’s user-facing name is “KDE Wallet” (“KWalletManager” names the app)',
    pattern: /\bKWallet\b/,
  },
]

/** Every every-OS wrong word in `key`'s value (any branch, macOS too) or its `@key` metadata. */
function everyOsWrongWords(key: string, value: string, metadata: unknown): string[] {
  const visible = visibleText(key, value)
  const described = metadata === undefined ? '' : JSON.stringify(metadata)
  return EVERY_OS_WRONG_WORDS.flatMap(({ rule, pattern }) => [
    ...(pattern.test(visible) ? [`${key}: ${rule}`] : []),
    ...(pattern.test(described) ? [`@${key}: ${rule}`] : []),
  ])
}

/** `<key> <branch>` → why that branch keeps a word the rules flag. */
const EXCEPTIONS: Readonly<Record<string, string>> = {
  'errors.listing.authRequiredEneedauth.suggestion other':
    'unreachable on Linux (ENEEDAUTH is a BSD errno), so it keeps the macOS text',
  'transfer.metadataClass.extendedAttributes windows':
    'open on Windows (no Windows metadata bodies yet), so it keeps the macOS text until they land',
}

/** Every wrong-OS word in `value`'s Windows and Linux branches, as `<key> <branch>: <rule>`. */
function wrongOsWords(key: string, value: string): string[] {
  const hits: string[] = []
  for (const { name, text } of platformBranches(value)) {
    if (name === null || name === 'macos') continue
    const visible = visibleText(key, text)
    const rules = [
      ...MACOS_WORDS.filter(({ pattern }) => pattern.test(visible)).map(({ rule }) => rule),
      ...SYSTEM_TOKENS.filter((token) => text.includes(token)).map((token) => `macOS-only token ${token}`),
      ...(name === 'windows' ? WINDOWS_WRONG_WORDS : LINUX_WRONG_WORDS)
        .filter(({ pattern }) => pattern.test(visible))
        .map(({ rule }) => rule),
    ]
    for (const rule of rules) hits.push(`${key} ${name}: ${rule}`)
  }
  return hits
}

/**
 * macOS things a plain value can't name, since it shows them on every OS: the branch rules'
 * macOS words (minus the key glyphs, `macKeyGlyphsInPlainValue`'s), the places and permissions
 * only macOS has, and the two words Windows says differently (style guide § "Platform vocabulary").
 */
const PLAIN_VALUE_MAC_WORDS: readonly WordRule[] = [
  ...MACOS_WORDS.filter(({ pattern }) => pattern !== MAC_KEY_GLYPH),
  { rule: 'macOS word “System Settings”', pattern: /\bSystem Settings\b/ },
  { rule: 'macOS word “Full Disk Access”', pattern: /\bFull Disk Access\b/i },
  { rule: 'macOS word “iCloud”', pattern: /\biCloud\b/ },
  { rule: 'macOS word “AirDrop”', pattern: /\bAirDrop\b/ },
  { rule: 'macOS word “Time Machine”', pattern: /\bTime Machine\b/ },
  { rule: 'macOS word “Apple silicon”', pattern: /\bApple silicon\b/i },
  { rule: 'macOS word “Applications folder”', pattern: /\bApplications folder\b/i },
  ...WINDOWS_WRONG_WORDS.filter(({ rule }) => !rule.startsWith('“Quit”')),
]

/** Every macOS word or token a plain (block-less) `value` shows to every OS, as `<key>: <rule>`. */
function macWordsInPlainValue(key: string, value: string): string[] {
  if (platformBranches(value).some(({ name }) => name !== null)) return []
  const visible = visibleText(key, value)
  return [
    ...PLAIN_VALUE_MAC_WORDS.filter(({ pattern }) => pattern.test(visible)).map(({ rule }) => rule),
    ...SYSTEM_TOKENS.filter((token) => value.includes(token)).map((token) => `macOS-only token ${token}`),
  ].map((rule) => `${key}: ${rule}`)
}

/**
 * Plain values that name a macOS thing and stay plain, because only a Mac ever shows them: a key,
 * or a key prefix ending in `.` or `*` (`macOnlyEntryFor`), → why no other OS reaches it. Most name the capability
 * (`apps/desktop/src-tauri/src/platform/capabilities.rs`) that hides the surface off macOS. A
 * value that a Windows or Linux user can see doesn't belong here: give it a `{platform, select}`
 * block (`src/lib/intl/DETAILS.md` § Platform variants).
 */
const MAC_ONLY_PLAIN_VALUES: Readonly<Record<string, string>> = {
  'aboutThis.':
    'states what macOS recorded (where-from, quarantine, receipts) or what a Mac file carries (Mach-O, Apple signatures), worded as recorded',
  'ai.local.notInstalled': '`localAi`: the bundled model runtime is the macOS ARM64 build only',
  'settings.ai.tooltipLocal': '`localAi`: the bundled model runtime is the macOS ARM64 build only',
  'settings.ai.tooltipLocalDisabled': '`localAi`: the bundled model runtime is the macOS ARM64 build only',
  'onboarding.stepAi.localTooltip': '`localAi`: the bundled model runtime is the macOS ARM64 build only',
  'askCmdr.tool.outcome.miss.needsDiskAccess': 'a macOS privacy gate (TCC) is the only source of this miss',
  'askCmdr.tool.tag.': '`colorTags`: the tag tool refuses off macOS',
  'commands.tagsToggle*': '`colorTags`: the tag commands exist only on macOS',
  'fileExplorer.tags.skipped': '`colorTags`: only macOS tags files',
  'settings.listing.showTags.description': '`colorTags`: the setting needs the capability',
  'commands.handler.openTerminalHere.': '`openTerminalHere`: the command exists only on macOS',
  'fileExplorer.functionKeyBar.terminalLabel': '`openTerminalHere`: the F9 slot exists only with the capability',
  'settings.behavior.openTerminalHereApp.description': '`openTerminalHere`: the setting needs the capability',
  'settings.navigationAndFileOps.card.terminal': '`openTerminalHere`: the card needs the capability',
  'commands.handler.getInfo.': '`getInfo`: Finder’s Get Info, macOS only',
  'settings.behavior.textEditorApp.description': '`textEditorChoice`: the setting needs the capability',
  'downloads.fda.': '`fullDiskAccess`: the Full Disk Access prompt is macOS only',
  'onboarding.fdaBadge.': '`fullDiskAccess`: the badge needs the capability',
  'onboarding.stepFda.': '`fullDiskAccess`: Linux and Windows skip the step',
  'onboarding.stepAi.banner*': '`fullDiskAccess`: the FDA-outcome banner is hidden without the capability',
  'askCmdr.wake.needsFullDiskAccess': 'set only while the macOS Full Disk Access question is open (`fda_gate`)',
  'search.coverage.deniedFullDiskAccess':
    '`fullDiskAccess`: shown only with the grant offer, which needs the capability',
  'search.coverage.setUpFullDiskAccess':
    '`fullDiskAccess`: shown only with the grant offer, which needs the capability',
  'settings.onboarding.fullDiskAccessChoice.label': '`fullDiskAccess`: the hidden setting needs the capability',
  'errors.write.trashRefused.suggestion.noFullDiskAccess': '`fullDiskAccess`: picked only where the capability is on',
  'errors.listing.tccRestricted.': 'a macOS privacy gate (TCC) is the only source of this reason',
  'errors.listing.emptyRootICloud.': 'iCloud Drive is found only under `~/Library/Mobile Documents`',
  'errors.provider.iCloud.': 'iCloud Drive is found only under `~/Library/Mobile Documents`',
  'errors.provider.macFuse.': 'found only by the macOS `statfs` fs type',
  'errors.provider.pCloudFuse.': 'found only under `/Volumes/pCloudDrive` or by the macOS `statfs` fs type',
  'errors.write.trashRefused.message.locked': 'the Locked flag (`uchg`) is read only by the macOS metadata body',
  'errors.write.trashRefused.suggestion.locked': 'the Locked flag (`uchg`) is read only by the macOS metadata body',
  'errors.mutation.fileLocked': 'the Locked flag (`uchg`) is read only by the macOS metadata body',
  'errors.mutation.sipProtected': 'System Integrity Protection is macOS',
  'transfer.systemLockedKept': '`schg` is read only by the macOS metadata body',
  'errors.mount.signInHelperStuck': '`NetAuthSysAgent` stalls only the macOS kernel mount',
  'servers.refusal.signInHelperStuck': '`NetAuthSysAgent` stalls only the macOS kernel mount',
  'fileExplorer.pane.directConnectionBlockedAfterUpdateToast': '`localNetworkPermission`: macOS only',
  'settings.network.permissionIntroSuffix': '`localNetworkPermission`: macOS only',
  'fileExplorer.navigation.useSavedPasswordMessage': 'offered only when the macOS Keychain holds a saved password',
  'fileExplorer.quickLookHint.': '`quickLook`: the hint shows only where Quick Look exists',
  'settings.fileExplorer.suppressQuickLookHint.': '`quickLook`: the setting needs the capability',
  'viewer.binaryWarning.bodyWithKeys': '`quickLook`: picked only while `file.quickLook` has a key',
  'viewer.binaryWarning.bodyWithQuickLookKey': '`quickLook`: picked only while `file.quickLook` has a key',
  'menu.file.quickLook': '`quickLook`: a macOS-only row in the bar and the right-click menu',
  'settings.fileViewer.suppressBinaryWarning.keywords': 'search keywords, never shown; “Quick Look” only adds a match',
  'main.dockPinNudge.': '`dockPin`: macOS only',
  'settings.behavior.dockPinNudgeOfferedAt.': '`dockPin`: the hidden setting needs the capability',
  'main.revealNudge.': '`revealHandler`: macOS only',
  'main.revealActivation.': '`revealHandler`: macOS only',
  'settings.revealHandler.': '`revealHandler`: the card needs the capability',
  'settings.navigationAndFileOps.card.showInFinder': '`revealHandler`: the card needs the capability',
  'settings.behavior.revealNudgeOfferedAt.': '`revealHandler`: the hidden setting needs the capability',
  'settings.behavior.revealActivationNoticeSeen.': '`revealHandler`: the setting needs the capability',
  'main.finderExtensionNudge.': '`finderExtension`: macOS only',
  'settings.finderExtension.': '`finderExtension`: the card needs the capability',
  'settings.behavior.finderExtensionNudgeOfferedAt.': '`finderExtension`: the hidden setting needs the capability',
  'main.oldMacos.': 'the old-macOS notice runs only on macOS (`startup-gates.ts`)',
  'settings.advanced.oldMacosNoticeShown.':
    'the old-macOS notice’s flag; the label is an open row in `docs/notes/windows-port/copy-decisions.md`',
  'main.cliCommand.': '`cliCommand`: macOS only',
  'notifications.permissionDenied': 'only macOS reports notifications as denied (`notifications.rs`)',
  'settings.mediaIndex.privacyNote':
    '`imageIndexing`: names the macOS engine (Vision); rewrite it with the Windows engine (#942)',
  'settings.mediaIndex.clip.notSupported':
    '`imageIndexing`: names the macOS engine’s hardware; rewrite it with the Windows engine (#942)',
  'settings.mediaIndex.parallelism.description':
    '`imageIndexing`: names the macOS engine’s hardware; rewrite it with the Windows engine (#942)',
  'settings.advanced.showSafeSaveFiles.description': '`safeSaveFiles`: the setting needs the capability',
  'shortcuts.conflict.reservedByMacOs': 'only an app-menu command (`appMenu`, macOS) is reserved this way',
  'shortcuts.section.macOs*': 'the badge marks app-menu commands (`appMenu`, macOS)',
  'shortcuts.conflict.systemShortcut': 'the macOS row of `system-reserved-keys.ts`; Windows and Linux have their own',
  'shortcuts.system.spotlight': 'the macOS row of `system-reserved-keys.ts`; Windows and Linux have their own',
  'shortcuts.system.finderSearch': 'the macOS row of `system-reserved-keys.ts`; Windows and Linux have their own',
  'updates.unwritable.': 'only the macOS installer reports an unwritable bundle',
  'updates.status.unwritableStandard': 'only the macOS installer reports an unwritable bundle',
  'updates.moveToApplicationsDialog.': 'only the macOS installer reports a translocated bundle',
}

/**
 * The `MAC_ONLY_PLAIN_VALUES` entry that covers `key`: the key itself, a prefix of it ending in
 * `.`, or one ending in `*` (the prefix before the star, for a family that shares no dot segment).
 */
function macOnlyEntryFor(key: string, entries: Readonly<Record<string, string>>): string | undefined {
  return Object.keys(entries).find(
    (entry) =>
      entry === key ||
      (entry.endsWith('.') && key.startsWith(entry)) ||
      (entry.endsWith('*') && key.startsWith(entry.slice(0, -1))),
  )
}

/** `[key]` when a plain (block-less) `value` shows a macOS key glyph to every OS, else `[]`. */
function macKeyGlyphsInPlainValue(key: string, value: string): string[] {
  return platformBranches(value).some(({ name, text }) => name === null && MAC_KEY_GLYPH.test(visibleText(key, text)))
    ? [key]
    : []
}

describe('macKeyGlyphsInPlainValue', () => {
  it('flags a typed macOS key glyph in a plain value', () => {
    expect(macKeyGlyphsInPlainValue('a.b', 'Copy files first with ⌘C.')).toEqual(['a.b'])
    expect(macKeyGlyphsInPlainValue('a.b', 'Press ⇧Space for Quick Look.')).toEqual(['a.b'])
  })

  it('leaves a key cap tag, a combo param, and a platform block alone', () => {
    expect(macKeyGlyphsInPlainValue('a.b', 'Copy files first with <key></key>.')).toEqual([])
    expect(macKeyGlyphsInPlainValue('a.b', 'Copy files first with {combo}.')).toEqual([])
    const block = '{platform, select, macos {Add a modifier (⌘ or ⇧)} other {Add a modifier (Ctrl or Shift)}}'
    expect(macKeyGlyphsInPlainValue('a.b', block)).toEqual([])
  })
})

describe('macWordsInPlainValue', () => {
  it('flags a macOS word, place, or token in a plain value', () => {
    expect(macWordsInPlainValue('a.b', 'It never leaves your Mac.')).toEqual(['a.b: macOS word “Mac”'])
    expect(macWordsInPlainValue('a.b', 'Requires Full Disk Access.')).toEqual(['a.b: macOS word “Full Disk Access”'])
    expect(macWordsInPlainValue('a.b', 'Moved to Trash')).toEqual([
      'a.b: “Trash” on Windows: “Delete” in a label, “the Recycle Bin” in a sentence',
    ])
    expect(macWordsInPlainValue('errors.x.suggestion', 'Open {system_settings}.')).toEqual([
      'errors.x.suggestion: macOS-only token {system_settings}',
    ])
  })

  it('leaves a platform block, a word inside a longer one, and a placeholder name alone', () => {
    const block = '{platform, select, macos {It never leaves your Mac.} other {It never leaves your computer.}}'
    expect(macWordsInPlainValue('a.b', block)).toEqual([])
    expect(macWordsInPlainValue('a.b', 'Machine learning runs on macOSX-free hardware')).toEqual([])
    expect(macWordsInPlainValue('a.b', 'Command palette…')).toEqual([])
    expect(macWordsInPlainValue('a.b', 'Opened {macName}')).toEqual([])
  })
})

describe('macOnlyEntryFor', () => {
  const entries = { 'main.dockPinNudge.': 'r', 'commands.tagsToggle*': 'r', 'notifications.permissionDenied': 'r' }

  it('matches a key, a dot prefix, and a star prefix', () => {
    expect(macOnlyEntryFor('notifications.permissionDenied', entries)).toBe('notifications.permissionDenied')
    expect(macOnlyEntryFor('main.dockPinNudge.title', entries)).toBe('main.dockPinNudge.')
    expect(macOnlyEntryFor('commands.tagsToggleRed.description', entries)).toBe('commands.tagsToggle*')
  })

  it('reads a key without a trailing dot or star as exact', () => {
    expect(macOnlyEntryFor('notifications.permissionDeniedAgain', entries)).toBeUndefined()
    expect(macOnlyEntryFor('main.dockPinNudgeOfferedAt', entries)).toBeUndefined()
  })
})

describe('wrongOsWords', () => {
  it('flags a macOS word in a Windows branch and a Windows word in a Linux one', () => {
    const value = '{platform, select, macos {Show in Finder} windows {Show in Finder} other {Move to the Recycle Bin}}'
    expect(wrongOsWords('commands.x.label', value)).toEqual([
      'commands.x.label windows: macOS word “Finder”',
      'commands.x.label other: “Recycle Bin” on Linux: “Trash”',
    ])
  })

  it('reads a Linux desktop branch as Linux', () => {
    const value = '{platform, select, macos {Show in Finder} linuxKde {Show in Finder} other {Show in file manager}}'
    expect(wrongOsWords('commands.x.label', value)).toEqual(['commands.x.label linuxKde: macOS word “Finder”'])
  })

  it('reads a raw block branch by branch and catches a macOS-only token', () => {
    const value = '{platform, select, macos {Open {system_settings}.} other {Open {system_settings}.}}'
    expect(wrongOsWords('errors.x.suggestion', value)).toEqual([
      'errors.x.suggestion other: macOS-only token {system_settings}',
    ])
  })

  it('leaves the macOS branch, a branch in its own words, and plain values alone', () => {
    const value = '{platform, select, macos {Show in Finder} windows {Move to the Recycle Bin} other {Move to Trash}}'
    expect(wrongOsWords('a.b', value)).toEqual([])
    expect(wrongOsWords('a.b', 'Show in Finder')).toEqual([])
  })
})

describe('everyOsWrongWords', () => {
  it('flags “file name” in any branch and in the @key metadata', () => {
    const value = '{platform, select, macos {No file names leave your Mac.} other {No filenames leave this computer.}}'
    const metadata = { description: 'Shown under the File name field.' }
    expect(everyOsWrongWords('a.b', value, metadata)).toEqual([
      'a.b: “file name”: Cmdr writes “filename”, one word, on every OS',
      '@a.b: “file name”: Cmdr writes “filename”, one word, on every OS',
    ])
  })

  it('leaves “filename” and a verb “file names” sitting across words alone', () => {
    expect(everyOsWrongWords('a.b', 'Copy filename', { description: 'The filenames column.' })).toEqual([])
    expect(everyOsWrongWords('a.b', 'Profile name', undefined)).toEqual([])
  })

  it('flags “KWallet” but not the KWalletManager app', () => {
    const value =
      '{platform, select, macos {Remember in Keychain} linuxKde {Remember in KWallet} other {Remember in keyring}}'
    expect(everyOsWrongWords('a.b', value, undefined)).toEqual([
      'a.b: “KWallet”: KDE’s user-facing name is “KDE Wallet” (“KWalletManager” names the app)',
    ])
    expect(everyOsWrongWords('a.b', 'Unlock it in KWalletManager.', undefined)).toEqual([])
  })
})

describe('the en catalog', () => {
  it('writes “filename” as one word, in values and @key descriptions', () => {
    const { messages, metadata } = loadCatalog('en')
    expect(Object.entries(messages).flatMap(([key, value]) => everyOsWrongWords(key, value, metadata[key]))).toEqual([])
  })

  it('has no wrong-OS word in a Windows or Linux branch', () => {
    const { messages } = loadCatalog('en')
    const hits = Object.entries(messages)
      .flatMap(([key, value]) => wrongOsWords(key, value))
      .filter((hit) => !Object.hasOwn(EXCEPTIONS, hit.slice(0, hit.indexOf(':'))))
    expect(hits).toEqual([])
  })

  it('types no macOS key glyph into a plain value', () => {
    const { messages } = loadCatalog('en')
    expect(Object.entries(messages).flatMap(([key, value]) => macKeyGlyphsInPlainValue(key, value))).toEqual([])
  })

  it('names no macOS thing in a plain value that Windows or Linux shows', () => {
    const { messages } = loadCatalog('en')
    const hits = Object.entries(messages)
      .filter(([key]) => macOnlyEntryFor(key, MAC_ONLY_PLAIN_VALUES) === undefined)
      .flatMap(([key, value]) => macWordsInPlainValue(key, value))
    expect(hits).toEqual([])
  })

  it('names only live keys in its Mac-only plain values, each with a reason', () => {
    const { messages } = loadCatalog('en')
    const covering = new Set(
      Object.entries(messages)
        .filter(([key, value]) => macWordsInPlainValue(key, value).length > 0)
        .map(([key]) => macOnlyEntryFor(key, MAC_ONLY_PLAIN_VALUES)),
    )
    expect(Object.keys(MAC_ONLY_PLAIN_VALUES).filter((entry) => !covering.has(entry))).toEqual([])
    expect(Object.entries(MAC_ONLY_PLAIN_VALUES).filter(([, reason]) => reason.trim().length < 10)).toEqual([])
  })

  it('names only live branches in its exceptions', () => {
    const { messages } = loadCatalog('en')
    const flagged = new Set(
      Object.entries(messages).flatMap(([key, value]) =>
        wrongOsWords(key, value).map((hit) => hit.slice(0, hit.indexOf(':'))),
      ),
    )
    expect(Object.keys(EXCEPTIONS).filter((entry) => !flagged.has(entry))).toEqual([])
  })
})
