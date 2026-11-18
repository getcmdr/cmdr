import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, it, expect, beforeAll, afterAll, onTestFinished } from 'vitest'
import type { NameRules } from '#lib/ipc/bindings.js'
import { withCapabilities, withPlatform } from '#lib/platform/test-platform.js'
import { _setLocaleForTests } from '#lib/intl/locale.js'
import {
  nameRefusal,
  validateName,
  validateNotEmpty,
  validatePathLength,
  validateDirectoryPath,
  validateExtensionChange,
  validateConflict,
  validateFilename,
  extensionsDifferMeaningfully,
  extensionChangeNeedsConfirmation,
} from './filename-validation'

// Validation messages resolve through the i18n catalog (`tString`); pin the base
// locale so the asserted en copy is deterministic.
beforeAll(() => {
  _setLocaleForTests('en-US')
})
afterAll(() => {
  _setLocaleForTests(null)
})

interface NameRulesSpec {
  cases: {
    name: string
    repeat?: number
    all?: string
    posix?: string
    smbShare?: string
    windows?: string
    character?: string
    reserved?: string
  }[]
}

const nameRulesSpec = JSON.parse(
  readFileSync(resolve(process.cwd(), '../../crates/cmdr-fs/src/name_rules_cases.json'), 'utf8'),
) as NameRulesSpec

/** The mirror of `cmdr_fs::validation::validate_filename`: both suites run the shared cases. */
describe('nameRefusal ↔ the backend`s `validate_filename`', () => {
  const rulesSets: NameRules[] = ['posix', 'smbShare', 'windows']
  it.each(nameRulesSpec.cases)('$name', (testCase) => {
    const name = testCase.name.repeat(testCase.repeat ?? 1)
    for (const rules of rulesSets) {
      const expected = testCase[rules] ?? testCase.all
      const refusal = nameRefusal(name, rules)
      expect(refusal?.kind ?? 'ok', `${JSON.stringify(testCase.name)} under ${rules}`).toBe(expected)
      if (refusal?.kind === 'disallowedCharacter') expect(refusal.character).toBe(testCase.character)
      if (refusal?.kind === 'reservedName') expect(refusal.name).toBe(testCase.reserved)
    }
  })
})

describe('validateName', () => {
  it('allows normal filenames', () => {
    expect(validateName('hello.txt', false, 'posix')).toEqual({ severity: 'ok', message: '' })
  })

  it('names the refused character, per file or folder', () => {
    expect(validateName('foo/bar', true, 'posix').message).toBe('Folder names here can’t contain “/”')
    expect(validateName('12:30', false, 'windows').message).toBe('Filenames here can’t contain “:”')
  })

  it('says a reserved device name is Windows`s', () => {
    expect(validateName('con.txt', false, 'smbShare').message).toBe(
      '“con” is reserved by Windows, so it can’t be a filename here',
    )
  })

  it('says a Windows drive refuses a trailing space or period', () => {
    expect(validateName('notes.', true, 'windows').message).toBe('Folder names here can’t end with a space or a period')
  })

  it('takes a 255-byte name and refuses a 256-byte one, counting bytes', () => {
    expect(validateName('a'.repeat(255), false, 'posix').severity).toBe('ok')
    // Each emoji is 4 bytes in UTF-8
    expect(validateName('\u{1F600}'.repeat(64), false, 'posix').severity).toBe('error')
  })
})

describe('validateNotEmpty', () => {
  it('rejects empty string', () => {
    expect(validateNotEmpty('').severity).toBe('error')
  })

  it('rejects whitespace-only', () => {
    expect(validateNotEmpty('   ').severity).toBe('error')
  })

  it('allows non-empty string', () => {
    expect(validateNotEmpty('a').severity).toBe('ok')
  })

  it('uses the file-vs-folder variant for the empty message', () => {
    expect(validateNotEmpty('', false).message).toBe('Filename can’t be empty')
    expect(validateNotEmpty('', true).message).toBe('Folder name can’t be empty')
  })
})

describe('validatePathLength', () => {
  it('allows short paths', () => {
    expect(validatePathLength('/Users/test', 'file.txt').severity).toBe('ok')
  })

  it('rejects paths at 1024 bytes', () => {
    const parent = '/a'.repeat(500) // 1000 bytes
    expect(validatePathLength(parent, 'a'.repeat(30)).severity).toBe('error')
  })

  it('handles trailing slash in parent', () => {
    expect(validatePathLength('/Users/test/', 'file.txt').severity).toBe('ok')
  })
})

describe('validateDirectoryPath', () => {
  it('rejects empty string', () => {
    const result = validateDirectoryPath('')
    expect(result.severity).toBe('error')
    expect(result.message).toBe('Path can’t be empty')
  })

  it('rejects whitespace-only string', () => {
    const result = validateDirectoryPath('   ')
    expect(result.severity).toBe('error')
    expect(result.message).toBe('Path can’t be empty')
  })

  it('rejects relative path', () => {
    const result = validateDirectoryPath('Documents/folder')
    expect(result.severity).toBe('error')
    expect(result.message).toBe('Path must be absolute (start with /)')
  })

  // The message names the forms the validator accepts on that OS: a drive letter or a share on
  // Windows, a leading slash elsewhere.
  it.each([
    ['macos', 'Path must be absolute (start with /)'],
    ['windows', 'Path must be absolute (start with a drive letter, like C:\\, or a share, like \\\\server\\share)'],
    ['linux', 'Path must be absolute (start with /)'],
  ] as const)('rejects a relative path on %s with that OS’s absolute forms', (platform, message) => {
    onTestFinished(withPlatform(platform))
    expect(validateDirectoryPath('Documents/folder')).toEqual({ severity: 'error', message })
    if (platform === 'windows') {
      expect(validateDirectoryPath('C:\\Users\\test_user_1').severity).toBe('ok')
      expect(validateDirectoryPath('\\\\server\\share').severity).toBe('ok')
    } else {
      expect(validateDirectoryPath('/home/test_user_1').severity).toBe('ok')
      expect(validateDirectoryPath('C:\\Users\\test_user_1')).toEqual({ severity: 'error', message })
    }
  })

  it('accepts a drive or UNC path, either slash, on Windows', () => {
    onTestFinished(withPlatform('windows'))
    expect(validateDirectoryPath('C:/Users/test_user_1').severity).toBe('ok')
    expect(validateDirectoryPath('C:\\Users\\test_user_1').severity).toBe('ok')
    expect(validateDirectoryPath('\\\\server\\share\\x').severity).toBe('ok')
    expect(validateDirectoryPath('/Users/test_user_1').severity).toBe('ok')
    expect(validateDirectoryPath('Documents\\folder').severity).toBe('error')
  })

  it('reads `\\` as a separator everywhere in the path on Windows', () => {
    onTestFinished(withPlatform('windows'))
    // The home shortcut and a drive-relative path, spelled the way Windows shows them.
    expect(validateDirectoryPath('~\\Documents').severity).toBe('ok')
    expect(validateDirectoryPath('\\Users\\test_user_1').severity).toBe('ok')
    // Names are measured one by one: a deep path of short names is fine, one long name isn't.
    const deep = 'C:\\' + Array.from({ length: 30 }, () => 'folder-name').join('\\')
    expect(validateDirectoryPath(deep).severity).toBe('ok')
    const longName = validateDirectoryPath(`C:\\Users\\${'a'.repeat(256)}\\x`)
    expect(longName.message).toMatch(/A folder name in the path is too long/)
  })

  /** #1118: `//server` names no share, so the grammar reads no folder there; say so before the backend does. */
  it.each(['\\\\server', '//server', '\\\\server\\', '//server/'])('asks for the share in %s on Windows', (path) => {
    onTestFinished(withPlatform('windows'))
    expect(validateDirectoryPath(path)).toEqual({
      severity: 'error',
      message: 'Add the share’s name after the server, like \\\\server\\share',
    })
  })

  it.each(['macos', 'linux'] as const)('keeps `//server` an ordinary absolute path on %s', (platform) => {
    onTestFinished(withPlatform(platform))
    expect(validateDirectoryPath('//server').severity).toBe('ok')
  })

  it.each(['macos', 'linux'] as const)('keeps `\\` a name character on %s', (platform) => {
    onTestFinished(withPlatform(platform))
    const oneName = validateDirectoryPath(`/x/${'a'.repeat(200)}\\${'a'.repeat(100)}`)
    expect(oneName.message).toMatch(/A folder name in the path is too long/)
    expect(validateDirectoryPath('~\\Documents').severity).toBe('error')
  })

  it.each(['macos', 'linux'] as const)('rejects a drive-shaped path as relative on %s', (platform) => {
    onTestFinished(withPlatform(platform))
    expect(validateDirectoryPath('C:/Users/test_user_1').severity).toBe('error')
  })

  it('rejects path with null byte', () => {
    const result = validateDirectoryPath('/Users/test\0/folder')
    expect(result.severity).toBe('error')
    expect(result.message).toBe('Path contains a null character')
  })

  it('rejects path at 1024 bytes', () => {
    const longPath = '/' + 'a'.repeat(1023)
    const result = validateDirectoryPath(longPath)
    expect(result.severity).toBe('error')
    expect(result.message).toMatch(/Path is too long/)
  })

  it('rejects path with a component past 255 bytes', () => {
    const longComponent = 'a'.repeat(256)
    const result = validateDirectoryPath(`/Users/${longComponent}/folder`)
    expect(result.severity).toBe('error')
    expect(result.message).toMatch(/A folder name in the path is too long/)
  })

  it('allows valid absolute path', () => {
    expect(validateDirectoryPath('/Users/test/Documents').severity).toBe('ok')
  })

  it('allows root path', () => {
    expect(validateDirectoryPath('/').severity).toBe('ok')
  })

  it('allows the bare home shortcut', () => {
    expect(validateDirectoryPath('~').severity).toBe('ok')
  })

  it('allows a home-rooted path', () => {
    expect(validateDirectoryPath('~/Documents/folder').severity).toBe('ok')
  })

  it('rejects a tilde not followed by a slash as relative', () => {
    const result = validateDirectoryPath('~foo')
    expect(result.severity).toBe('error')
    expect(result.message).toBe('Path must be absolute (start with /)')
  })

  it('handles trailing slashes', () => {
    expect(validateDirectoryPath('/Users/test/').severity).toBe('ok')
  })

  it('handles double slashes', () => {
    expect(validateDirectoryPath('/Users//test').severity).toBe('ok')
  })

  it('counts multi-byte characters correctly', () => {
    // Each emoji is 4 bytes
    const emojiDir = '\u{1F600}'.repeat(64) // 256 bytes
    const result = validateDirectoryPath(`/Users/${emojiDir}`)
    expect(result.severity).toBe('error')
    expect(result.message).toMatch(/A folder name in the path is too long/)
  })

  it('allows path just under 1024 bytes', () => {
    // Build a long path from many short segments to avoid per-component limit
    const segment = 'a'.repeat(100)
    const path = ('/' + segment).repeat(10) + '/' + 'b'.repeat(12) // 10 * 101 + 13 = 1023 bytes
    expect(validateDirectoryPath(path).severity).toBe('ok')
  })
})

describe('extensionsDifferMeaningfully', () => {
  it('reads extensions by the product rule, so words after a dot are part of the name', () => {
    expect(extensionsDifferMeaningfully('Smlouva v. 2 final', 'Smlouva v. 3 final')).toBe(false)
    expect(extensionsDifferMeaningfully('J. Novak - notes', 'J. Novakova - notes')).toBe(false)
  })

  it('treats a compound tar as one extension', () => {
    expect(extensionsDifferMeaningfully('src.tar.gz', 'source.tar.gz')).toBe(false)
    expect(extensionsDifferMeaningfully('src.tar.gz', 'src.gz')).toBe(true)
  })

  it('detects a real extension change', () => {
    expect(extensionsDifferMeaningfully('file.txt', 'file.json')).toBe(true)
  })

  it('ignores case-only changes', () => {
    expect(extensionsDifferMeaningfully('photo.JPG', 'photo.jpg')).toBe(false)
    expect(extensionsDifferMeaningfully('archive.TAR.GZ', 'archive.TAR.gz')).toBe(false)
  })

  it('ignores no-op changes', () => {
    expect(extensionsDifferMeaningfully('file.txt', 'renamed.txt')).toBe(false)
  })

  it('ignores changes within the JPEG group', () => {
    expect(extensionsDifferMeaningfully('photo.jpeg', 'photo.jpg')).toBe(false)
    expect(extensionsDifferMeaningfully('photo.jpg', 'photo.jpe')).toBe(false)
    expect(extensionsDifferMeaningfully('photo.jfif', 'photo.JPEG')).toBe(false)
  })

  it('ignores changes within the TIFF group', () => {
    expect(extensionsDifferMeaningfully('scan.tif', 'scan.tiff')).toBe(false)
  })

  it('ignores changes within the HTML group', () => {
    expect(extensionsDifferMeaningfully('page.htm', 'page.html')).toBe(false)
  })

  it('ignores changes within the YAML group', () => {
    expect(extensionsDifferMeaningfully('config.yml', 'config.yaml')).toBe(false)
  })

  it('ignores changes within the MPEG group', () => {
    expect(extensionsDifferMeaningfully('clip.mpg', 'clip.mpeg')).toBe(false)
  })

  it('ignores changes within the MIDI group', () => {
    expect(extensionsDifferMeaningfully('song.mid', 'song.midi')).toBe(false)
  })

  it('ignores changes within the AIFF group', () => {
    expect(extensionsDifferMeaningfully('sound.aif', 'sound.aiff')).toBe(false)
  })

  it('ignores changes within the QuickTime group', () => {
    expect(extensionsDifferMeaningfully('movie.qt', 'movie.mov')).toBe(false)
  })

  it('ignores changes within the markdown/text group', () => {
    expect(extensionsDifferMeaningfully('notes.md', 'notes.txt')).toBe(false)
    expect(extensionsDifferMeaningfully('notes.markdown', 'notes.md')).toBe(false)
    expect(extensionsDifferMeaningfully('notes.TXT', 'notes.Markdown')).toBe(false)
  })

  it('flags changes that cross groups', () => {
    expect(extensionsDifferMeaningfully('photo.jpg', 'photo.tif')).toBe(true)
    expect(extensionsDifferMeaningfully('notes.md', 'notes.html')).toBe(true)
  })

  it('flags adding an extension to a name that had none', () => {
    expect(extensionsDifferMeaningfully('Makefile', 'Makefile.txt')).toBe(true)
  })

  it('flags removing an extension', () => {
    expect(extensionsDifferMeaningfully('readme.txt', 'readme')).toBe(true)
  })

  it('treats a no-extension to no-extension rename as no change', () => {
    expect(extensionsDifferMeaningfully('Makefile', 'Dockerfile')).toBe(false)
  })

  it('trims the new name before comparing', () => {
    expect(extensionsDifferMeaningfully('photo.jpg', '  photo.jpeg  ')).toBe(false)
  })

  it('treats a dotfile without extension as no extension', () => {
    // '.gitignore' has no extension, so renaming to .gitkeep is also no extension
    expect(extensionsDifferMeaningfully('.gitignore', '.gitkeep')).toBe(false)
  })

  it('uses only the last extension on multi-dot names', () => {
    expect(extensionsDifferMeaningfully('archive.tar.gz', 'archive.tar.bz2')).toBe(true)
    expect(extensionsDifferMeaningfully('photo.backup.jpeg', 'photo.backup.jpg')).toBe(false)
  })
})

describe('validateExtensionChange', () => {
  it('allows when setting is yes', () => {
    expect(validateExtensionChange('file.txt', 'file.json', 'yes').severity).toBe('ok')
  })

  it('errors when setting is no and ext changed', () => {
    const result = validateExtensionChange('file.txt', 'file.json', 'no')
    expect(result.severity).toBe('error')
  })

  it('allows when setting is no but ext unchanged', () => {
    expect(validateExtensionChange('file.txt', 'renamed.txt', 'no').severity).toBe('ok')
  })

  it('allows when setting is ask (dialog handles it)', () => {
    expect(validateExtensionChange('file.txt', 'file.json', 'ask').severity).toBe('ok')
  })

  it('allows case-only extension change when setting is no', () => {
    expect(validateExtensionChange('photo.JPG', 'photo.jpg', 'no').severity).toBe('ok')
  })

  it('names the old extension with its dot in the blocked message', () => {
    expect(validateExtensionChange('file.txt', 'file.json', 'no').message).toContain('“.txt”')
  })

  it('does not block a file with no extension under the product rule', () => {
    expect(validateExtensionChange('Smlouva v. 2 final', 'Smlouva v. 3', 'no').severity).toBe('ok')
  })

  it('still errors on real extension change when setting is no', () => {
    expect(validateExtensionChange('file.txt', 'file.json', 'no').severity).toBe('error')
  })

  it('allows equivalent-extension changes when setting is no', () => {
    expect(validateExtensionChange('photo.jpeg', 'photo.jpg', 'no').severity).toBe('ok')
    expect(validateExtensionChange('notes.md', 'notes.txt', 'no').severity).toBe('ok')
  })

  it('lets a folder change its "extension" freely: a dot in a folder name is just part of the name', () => {
    expect(validateExtensionChange('release.v1.2', 'release.v1.3', 'no', 'folder').severity).toBe('ok')
    expect(validateExtensionChange('photos.2025', 'photos', 'no', 'folder').severity).toBe('ok')
  })

  it('still guards a folder whose extension makes it a macOS package', () => {
    onTestFinished(withCapabilities('appBundles'))
    expect(validateExtensionChange('Cmdr.app', 'Cmdr.bak', 'no', 'package').severity).toBe('error')
    expect(validateExtensionChange('Plugins', 'Plugins.bundle', 'no', 'folder').severity).toBe('error')
  })
})

describe('extensionChangeNeedsConfirmation', () => {
  it('asks for a file whose extension changes meaningfully', () => {
    expect(extensionChangeNeedsConfirmation('file.txt', 'file.json', 'file')).toBe(true)
  })

  it('never asks for a plain folder, dot or no dot', () => {
    expect(extensionChangeNeedsConfirmation('release.v1.2', 'release.v1.3', 'folder')).toBe(false)
    expect(extensionChangeNeedsConfirmation('my.project', 'my-project', 'folder')).toBe(false)
  })

  it('never asks for a folder the backend says is plain, even with a listed suffix', () => {
    // `isPackage` is false on a folder named like a bundle only when it's not one
    // to macOS (a symlink, say): the entry outranks the name.
    expect(extensionChangeNeedsConfirmation('Cmdr.app', 'Cmdr.old', 'folder')).toBe(false)
  })

  it('asks for a folder that stops or starts being a macOS package', () => {
    onTestFinished(withCapabilities('appBundles'))
    expect(extensionChangeNeedsConfirmation('Cmdr.app', 'Cmdr.old', 'package')).toBe(true)
    expect(extensionChangeNeedsConfirmation('Library', 'Library.photoslibrary', 'folder')).toBe(true)
    expect(extensionChangeNeedsConfirmation('Cmdr.APP', 'Cmdr', 'package')).toBe(true)
  })

  it('asks for a package macOS reported that the list does not know', () => {
    expect(extensionChangeNeedsConfirmation('Sketch.myBundle', 'Sketch.old', 'package')).toBe(true)
    expect(extensionChangeNeedsConfirmation('Sketch.myBundle', 'Sketch', 'package')).toBe(true)
    expect(extensionChangeNeedsConfirmation('Sketch.myBundle', 'Sketch v2.myBundle', 'package')).toBe(false)
  })

  it('does not ask for a package folder renamed within its extension', () => {
    expect(extensionChangeNeedsConfirmation('Cmdr.app', 'Cmdr 2.app', 'package')).toBe(false)
    expect(extensionChangeNeedsConfirmation('Cmdr.app', 'Cmdr.APP', 'package')).toBe(false)
  })
})

describe('validateConflict', () => {
  it('warns on case-insensitive match with a different sibling', () => {
    const result = validateConflict('README.md', ['readme.md', 'other.txt'], 'old.txt')
    expect(result.severity).toBe('warning')
    expect(result.message).toBe('“README.md” already exists in this folder')
  })

  it('no warning for case-only rename of same file', () => {
    const result = validateConflict('README.md', ['readme.md'], 'readme.md')
    expect(result.severity).toBe('ok')
  })

  it('no warning when no conflict', () => {
    const result = validateConflict('unique.txt', ['other.txt', 'file.md'], 'original.txt')
    expect(result.severity).toBe('ok')
  })

  it('trims before comparing', () => {
    const result = validateConflict('  conflict.txt  ', ['conflict.txt'], 'original.txt')
    expect(result.severity).toBe('warning')
  })
})

describe('validateFilename', () => {
  const parentPath = '/Users/test'
  const siblings: string[] = []

  it('returns ok for valid rename', () => {
    const result = validateFilename('newname.txt', 'old.txt', parentPath, siblings, 'yes', 'posix')
    expect(result.severity).toBe('ok')
  })

  it('returns error for empty name', () => {
    const result = validateFilename('', 'old.txt', parentPath, siblings, 'yes', 'posix')
    expect(result.severity).toBe('error')
  })

  it('returns error for disallowed chars', () => {
    const result = validateFilename('bad/name', 'old.txt', parentPath, siblings, 'yes', 'posix')
    expect(result.severity).toBe('error')
  })

  it('returns warning for conflict', () => {
    const result = validateFilename('conflict.txt', 'old.txt', parentPath, ['conflict.txt'], 'yes', 'posix')
    expect(result.severity).toBe('warning')
  })

  it('returns first error before warnings', () => {
    // Empty AND conflicting: error takes precedence
    const result = validateFilename('', 'old.txt', parentPath, [''], 'yes', 'posix')
    expect(result.severity).toBe('error')
  })

  it('applies the folder`s name rules', () => {
    expect(validateFilename('12:30.txt', 'old.txt', parentPath, siblings, 'yes', 'posix').severity).toBe('ok')
    expect(validateFilename('12:30.txt', 'old.txt', parentPath, siblings, 'yes', 'windows').severity).toBe('error')
  })

  it('validates extension change with no setting', () => {
    const result = validateFilename('file.json', 'file.txt', parentPath, siblings, 'no', 'posix')
    expect(result.severity).toBe('error')
  })
})
