import { describe, expect, it } from 'vitest'
import { classifyUaFamily, resolveUaFamily } from './user-agent'

const macSafari = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15'
const windowsEdge = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/131.0'
const linuxFirefox = 'Mozilla/5.0 (X11; Linux x86_64; rv:133.0) Gecko/20100101 Firefox/133.0'
const ubuntuFirefox = 'Mozilla/5.0 (X11; Ubuntu; Linux i686)'
const android = 'Mozilla/5.0 (Linux; Android 13; Pixel 7)'
const chromeOs = 'Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36'

describe('classifyUaFamily for a macOS download', () => {
  it('classifies a Mac browser UA as human (Macintosh / Mac OS markers)', () => {
    expect(classifyUaFamily(macSafari, 'macos')).toBe('human')
    expect(classifyUaFamily('Mozilla/5.0 (Mac OS X) Safari', 'macos')).toBe('human')
  })

  it('classifies Homebrew and curl/wget installs as human', () => {
    expect(classifyUaFamily('Homebrew/4.1.0 (Macintosh; arm64)', 'macos')).toBe('human')
    expect(classifyUaFamily('curl/8.4.0', 'macos')).toBe('human')
    expect(classifyUaFamily('Wget/1.21.4', 'macos')).toBe('human')
  })

  it('classifies non-macOS UAs (Windows / Android / Linux / X11) as bot, the provable exclusion', () => {
    expect(classifyUaFamily(windowsEdge, 'macos')).toBe('bot')
    expect(classifyUaFamily(android, 'macos')).toBe('bot')
    expect(classifyUaFamily(linuxFirefox, 'macos')).toBe('bot')
    expect(classifyUaFamily(ubuntuFirefox, 'macos')).toBe('bot')
  })

  it('classifies a NULL, empty, or unrecognized UA as unknown (never excluded)', () => {
    expect(classifyUaFamily(null, 'macos')).toBe('unknown')
    expect(classifyUaFamily(undefined, 'macos')).toBe('unknown')
    expect(classifyUaFamily('', 'macos')).toBe('unknown')
    expect(classifyUaFamily('SomeRandomFetcher/2.0', 'macos')).toBe('unknown')
  })

  it('lets a Mac marker win over a non-mac one, so a Mac-claiming UA is never excluded', () => {
    // Contrived, but documents the precedence: the artifact's own markers are checked first.
    expect(classifyUaFamily('Macintosh; also mentions Windows', 'macos')).toBe('human')
  })

  it('is case-insensitive', () => {
    expect(classifyUaFamily('MOZILLA/5.0 (MACINTOSH)', 'macos')).toBe('human')
    expect(classifyUaFamily('mozilla/5.0 (windows nt 10.0)', 'macos')).toBe('bot')
  })
})

describe('classifyUaFamily for a Windows download', () => {
  it('classifies a Windows browser, curl, and wget as human', () => {
    expect(classifyUaFamily(windowsEdge, 'windows')).toBe('human')
    expect(classifyUaFamily('curl/8.9.1', 'windows')).toBe('human')
    expect(classifyUaFamily('Wget/1.21.4', 'windows')).toBe('human')
  })

  it('classifies a Mac, Linux, or Android client as bot: it cannot run the installer', () => {
    expect(classifyUaFamily(macSafari, 'windows')).toBe('bot')
    expect(classifyUaFamily('Homebrew/4.1.0 (Macintosh; arm64)', 'windows')).toBe('bot')
    expect(classifyUaFamily(linuxFirefox, 'windows')).toBe('bot')
    expect(classifyUaFamily(android, 'windows')).toBe('bot')
  })

  it('leaves an unrecognized UA unknown', () => {
    expect(classifyUaFamily('SomeRandomFetcher/2.0', 'windows')).toBe('unknown')
  })
})

describe('classifyUaFamily for a Linux download', () => {
  it('classifies a Linux or X11 desktop browser, ChromeOS, curl, and wget as human', () => {
    expect(classifyUaFamily(linuxFirefox, 'linux')).toBe('human')
    expect(classifyUaFamily(ubuntuFirefox, 'linux')).toBe('human')
    expect(classifyUaFamily(chromeOs, 'linux')).toBe('human')
    expect(classifyUaFamily('curl/8.4.0', 'linux')).toBe('human')
  })

  it('classifies Android as bot even though its UA says Linux', () => {
    expect(classifyUaFamily(android, 'linux')).toBe('bot')
  })

  it('classifies a Mac or Windows client as bot', () => {
    expect(classifyUaFamily(macSafari, 'linux')).toBe('bot')
    expect(classifyUaFamily(windowsEdge, 'linux')).toBe('bot')
  })
})

describe('resolveUaFamily', () => {
  it('trusts the family stored at write time, so a cleared UA keeps its classification', () => {
    expect(resolveUaFamily({ uaFamily: 'human', userAgent: null, os: 'macos' })).toBe('human')
    expect(resolveUaFamily({ uaFamily: 'bot', userAgent: null, os: 'macos' })).toBe('bot')
  })

  it("classifies the raw UA against the row's artifact OS for rows written before the column existed", () => {
    expect(resolveUaFamily({ uaFamily: null, userAgent: 'Mozilla/5.0 (Macintosh)', os: 'macos' })).toBe('human')
    expect(resolveUaFamily({ uaFamily: null, userAgent: 'Mozilla/5.0 (Windows NT 10.0)', os: 'macos' })).toBe('bot')
    expect(resolveUaFamily({ uaFamily: null, userAgent: 'Mozilla/5.0 (Windows NT 10.0)', os: 'windows' })).toBe('human')
  })

  it('reads a row without a known OS as a macOS download, the only kind before the column', () => {
    expect(resolveUaFamily({ uaFamily: null, userAgent: 'Mozilla/5.0 (Macintosh)', os: null })).toBe('human')
  })

  it('falls back to unknown for a pre-column row whose UA the sweep already cleared', () => {
    expect(resolveUaFamily({ uaFamily: null, userAgent: null, os: 'macos' })).toBe('unknown')
  })

  it('ignores an unrecognized stored value rather than trusting it blindly', () => {
    expect(resolveUaFamily({ uaFamily: 'nonsense', userAgent: 'Mozilla/5.0 (Macintosh)', os: 'macos' })).toBe('human')
  })
})
