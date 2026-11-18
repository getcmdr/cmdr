/**
 * The card's lines that a Linux machine reaches: `/` is the startup disk's row there too
 * (`cmdr-about`'s `ROOT_STANDARD_FOLDERS`), and an xdg download record with no address reads the
 * no-source line. Each OS reads its own words (`docs/style-guide.md` § "Platform vocabulary"),
 * so neither names macOS off a Mac (#1235's sweep).
 */
import { afterEach, describe, expect, it } from 'vitest'
import { tString } from '#lib/intl/messages.svelte.js'
import { withPlatform } from '#lib/platform/test-platform.js'

let undo: (() => void) | null = null
afterEach(() => {
  undo?.()
  undo = null
})

describe('the card off macOS', () => {
  it('names the system disk, never the startup disk or macOS, on Linux', () => {
    undo = withPlatform('linux')
    expect(tString('aboutThis.standardFolder.startupDisk')).toBe('The top of the system disk')
    expect(tString('aboutThis.removal.standardFolder.startupDisk')).toBe(
      'It holds everything on the system disk, the operating system included.',
    )
  })

  it('keeps the Mac wording on macOS', () => {
    undo = withPlatform('macos')
    expect(tString('aboutThis.standardFolder.startupDisk')).toBe('The top of the startup disk')
    expect(tString('aboutThis.removal.standardFolder.startupDisk')).toBe(
      'It holds everything on the startup disk, macOS included.',
    )
  })

  it('says a download record has no address without naming macOS on Linux', () => {
    undo = withPlatform('linux')
    const text = tString('aboutThis.download.noSource', { hasApp: 'no', hasDate: 'no', app: '' })
    expect(text).not.toContain('macOS')
    expect(text).toBe('This carries a record of being downloaded, without saying from where.')
  })

  it('promises nothing about Mail or AirDrop off macOS, where neither records a download', () => {
    for (const os of ['windows', 'linux'] as const) {
      undo?.()
      undo = withPlatform(os)
      const text = tString('ai.cloudConsent.aboutThis.item.download')
      expect(text).not.toMatch(/Mail|AirDrop/)
    }
    undo?.()
    undo = withPlatform('macos')
    expect(tString('ai.cloudConsent.aboutThis.item.download')).toContain('AirDrop')
  })
})
