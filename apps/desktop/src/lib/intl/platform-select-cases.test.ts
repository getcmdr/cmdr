/**
 * The shared resolution table (`platform-select-cases.json`) through every TypeScript reader of
 * a platform block: the pure resolver, the raw splitter, `t()` (ICU's select), and `getMessage()`.
 * The boot guard, the native-strings generator, and Rust read the same file in their own tests.
 */
import { afterEach, describe, expect, it, onTestFinished } from 'vitest'
import { withConventions, withPlatform } from '#lib/platform/test-platform.js'
import { _clearCompiledCacheForTests, _setCatalogForTests, getMessage, tString } from './messages.svelte'
import { _setLocaleForTests } from './locale'
import {
  PLATFORM_SELECT_READERS,
  pickPlatformBranch,
  selectReaderFor,
  type PlatformSelectReader,
} from './platform-select'
import cases from './platform-select-cases.json'

type Platform = 'macos' | 'windows' | 'linux'
type LinuxDesktop = 'gnome' | 'kde' | 'other'

const SYSTEMS = cases.systems as {
  platform: Platform
  linuxDesktop: LinuxDesktop | null
  reader: PlatformSelectReader
}[]
const BLOCKS = cases.blocks as { name: string; value: string; reads: Record<PlatformSelectReader, string> }[]

const TEST_LOCALE = 'zz-ZZ'

afterEach(() => {
  _setLocaleForTests(null)
  _setCatalogForTests(TEST_LOCALE, null)
  _clearCompiledCacheForTests()
})

/** Moves this window to `system`: the platform, and the conventions of the desktop it names. */
function enter(system: (typeof SYSTEMS)[number]): void {
  onTestFinished(withPlatform(system.platform))
  const desktop = system.linuxDesktop
  if (desktop !== null) onTestFinished(withConventions(desktop === 'other' ? 'otherLinux' : desktop))
}

describe('platform-select-cases.json', () => {
  it('names every reader, in systems and in each block', () => {
    expect(new Set(SYSTEMS.map(({ reader }) => reader))).toEqual(new Set(PLATFORM_SELECT_READERS))
    for (const { reads } of BLOCKS) expect(Object.keys(reads).sort()).toEqual([...PLATFORM_SELECT_READERS].sort())
  })

  it.each(SYSTEMS)('$platform on desktop $linuxDesktop reads as $reader', ({ platform, linuxDesktop, reader }) => {
    expect(selectReaderFor(platform, linuxDesktop)).toBe(reader)
  })

  describe.each(BLOCKS)('$name', ({ value, reads }) => {
    it('the raw splitter picks each reader’s text', () => {
      for (const [reader, text] of Object.entries(reads)) {
        expect(pickPlatformBranch(value, reader as PlatformSelectReader)).toBe(text)
      }
    })

    it.each(SYSTEMS)('t() and getMessage() on $platform/$linuxDesktop read the table', (system) => {
      _setCatalogForTests(TEST_LOCALE, { 'transfer.trash': value, 'errors.write.fileLocked.message': value })
      _setLocaleForTests(TEST_LOCALE)
      enter(system)
      expect(tString('transfer.trash')).toBe(reads[system.reader])
      expect(getMessage('errors.write.fileLocked.message')).toBe(reads[system.reader])
    })
  })

  it('withPlatform(linux, desktop) moves the reader too, and its undo restores the neutral desktop', () => {
    _setCatalogForTests(TEST_LOCALE, { 'transfer.trash': BLOCKS[3].value })
    _setLocaleForTests(TEST_LOCALE)
    const undo = withPlatform('linux', 'kde')
    expect(tString('transfer.trash')).toBe(BLOCKS[3].reads.linuxKde)
    undo()
    expect(tString('transfer.trash')).toBe(BLOCKS[3].reads.linux)
  })
})
