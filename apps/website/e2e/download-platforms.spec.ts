import { readFileSync } from 'node:fs'
import { test, expect, type Page } from '@playwright/test'
import {
  chooseDownload,
  detectVisitorArch,
  detectVisitorOs,
  formatOsList,
  newsletterLabel,
  offeredPlatforms,
  parsePublicDownloadOses,
  type Os,
  type VisitorNavigator,
} from '../src/lib/download-platforms'
import type { UpdateManifest } from '../src/lib/release-downloads'
import { emulateVisitor, visitors, type Visitor } from './visitor'

// The download section and buttons per visitor OS. The first half tests the pure rules in
// `src/lib/download-platforms.ts` (no page). The second checks the built site: the home page offers what's
// public today (macOS), and `/download-fixture` renders a release that ships every OS.

const mb = 1024 * 1024
const everyOsManifest: UpdateManifest = {
  version: '1.2.0',
  downloads: [
    { os: 'macos', arch: 'aarch64', kind: 'dmg', file: 'Cmdr_1.2.0_aarch64.dmg', size: 50 * mb },
    { os: 'macos', arch: 'x86_64', kind: 'dmg', file: 'Cmdr_1.2.0_x64.dmg', size: 53 * mb },
    { os: 'macos', arch: 'universal', kind: 'dmg', file: 'Cmdr_1.2.0_universal.dmg', size: 91 * mb },
    { os: 'macos', arch: 'universal', kind: 'pkg', file: 'Cmdr_1.2.0_universal.pkg', size: 91 * mb },
    { os: 'windows', arch: 'x86_64', kind: 'msix', file: 'Cmdr_1.2.0_x64.msix', size: 30 * mb },
    { os: 'windows', arch: 'x86_64', kind: 'nsis', file: 'Cmdr_1.2.0_x64-setup.exe', size: 21 * mb },
    { os: 'windows', arch: '', kind: 'group-policy', file: 'cmdr-group-policy-1.2.0.zip', size: 1 },
    { os: 'linux', arch: 'aarch64', kind: 'deb', file: 'cmdr_1.2.0_arm64.deb', size: 24 * mb },
    { os: 'linux', arch: 'x86_64', kind: 'appimage', file: 'Cmdr_1.2.0_amd64.AppImage', size: 80 * mb },
    { os: 'linux', arch: 'aarch64', kind: 'appimage', file: 'Cmdr_1.2.0_aarch64.AppImage', size: 78 * mb },
  ],
}
const fileUrl = (d: { file: string }) => `https://mirror.test/${d.file}`
const oses = (...list: Os[]) => new Set<Os>(list)

test.describe('download platforms: which OSes the site offers', () => {
  test('reads PUBLIC_DOWNLOAD_OSES the way the API server does', () => {
    const toml = (value: string) => `[vars]\nPRODUCT_NAME = "Cmdr"\nPUBLIC_DOWNLOAD_OSES = "${value}"\n`
    expect([...parsePublicDownloadOses(toml('macos'))]).toEqual(['macos'])
    expect([...parsePublicDownloadOses(toml('macos,windows'))]).toEqual(['macos', 'windows'])
    expect([...parsePublicDownloadOses(toml(' windows , linux '))]).toEqual(['macos', 'windows', 'linux'])
    // macOS is always public, whatever the var says, and an unknown OS is ignored.
    expect([...parsePublicDownloadOses(toml(''))]).toEqual(['macos'])
    expect([...parsePublicDownloadOses(toml('solaris'))]).toEqual(['macos'])
    expect([...parsePublicDownloadOses('[vars]\nPRODUCT_NAME = "Cmdr"\n')]).toEqual(['macos'])
    // A commented-out line doesn't count.
    expect([...parsePublicDownloadOses('# PUBLIC_DOWNLOAD_OSES = "macos,windows"\n')]).toEqual(['macos'])
  })

  test('a release carrying Windows files offers only macOS until Windows is public', () => {
    const offered = offeredPlatforms(everyOsManifest, oses('macos'), fileUrl)
    expect(offered.map((p) => p.os)).toEqual(['macos'])
  })

  test('a public OS the release carries no file of stays off', () => {
    const macOnly: UpdateManifest = {
      ...everyOsManifest,
      downloads: everyOsManifest.downloads?.filter((d) => d.os === 'macos'),
    }
    expect(offeredPlatforms(macOnly, oses('macos', 'windows', 'linux'), fileUrl).map((p) => p.os)).toEqual(['macos'])
  })

  test('each public OS offers its website files in order, with names, links, and sizes', () => {
    const [mac, windows, linux] = offeredPlatforms(everyOsManifest, oses('macos', 'windows', 'linux'), fileUrl)
    expect(mac.options.map((o) => o.label)).toEqual(['Apple Silicon', 'Intel', 'Universal'])
    expect(mac.defaultArch).toBe('universal')
    // The MSIX is the Store's, and the policy zip is for admins: only the installer is a website download.
    expect(windows.options).toEqual([
      {
        arch: 'x86_64',
        kind: 'nsis',
        label: 'Installer · x64',
        url: 'https://mirror.test/Cmdr_1.2.0_x64-setup.exe',
        size: '21 MB',
      },
    ])
    expect(linux.options.map((o) => o.label)).toEqual(['AppImage · ARM64', 'AppImage · x86_64', '.deb · ARM64'])
    expect(linux.defaultArch).toBe('x86_64')
  })

  test('a manifest from before `downloads` still offers the three DMGs', () => {
    const old: UpdateManifest = { version: '0.50.0', dmgSizes: { aarch64: 1 * mb, x86_64: 2 * mb, universal: 3 * mb } }
    const offered = offeredPlatforms(old, oses('macos', 'windows'), fileUrl)
    expect(offered.map((p) => p.os)).toEqual(['macos'])
    expect(offered[0].options.map((o) => o.url)).toEqual([
      'https://mirror.test/Cmdr_0.50.0_aarch64.dmg',
      'https://mirror.test/Cmdr_0.50.0_x64.dmg',
      'https://mirror.test/Cmdr_0.50.0_universal.dmg',
    ])
  })

  test('copy follows the offered OSes', () => {
    expect(formatOsList(['macos'])).toBe('macOS')
    expect(formatOsList(['macos', 'windows'])).toBe('macOS and Windows')
    expect(formatOsList(['macos', 'windows', 'linux'])).toBe('macOS, Windows, and Linux')
    expect(newsletterLabel(['macos'])).toBe('Windows coming soon. Linux in alpha (self-build for now). Get notified:')
    expect(newsletterLabel(['macos', 'windows'])).toBe('Linux in alpha (self-build for now). Get notified:')
    expect(newsletterLabel(['macos', 'windows', 'linux'])).toBe('Get an email when a new version is out:')
  })
})

function navigatorOf(v: Visitor): VisitorNavigator {
  return {
    userAgent: v.userAgent,
    platform: v.platform,
    maxTouchPoints: v.maxTouchPoints ?? 0,
    userAgentData: v.hintsPlatform === null ? undefined : { platform: v.hintsPlatform, mobile: v.mobile ?? false },
  }
}

test.describe('download platforms: detecting the visitor', () => {
  test('names the desktop OS from Client Hints, the user agent, or navigator.platform', () => {
    expect(detectVisitorOs(navigatorOf(visitors.macArm))).toBe('macos')
    expect(detectVisitorOs(navigatorOf(visitors.windows))).toBe('windows')
    expect(detectVisitorOs(navigatorOf(visitors.linux))).toBe('linux')
    expect(detectVisitorOs(navigatorOf(visitors.linuxArm))).toBe('linux')
    // Safari and Firefox have no Client Hints: the user agent decides.
    const safari =
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15'
    expect(detectVisitorOs({ userAgent: safari, platform: 'MacIntel', maxTouchPoints: 0 })).toBe('macos')
    expect(
      detectVisitorOs({
        userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:140.0) Gecko/20100101 Firefox/140.0',
      }),
    ).toBe('windows')
    // An unknown user agent falls back to navigator.platform.
    expect(detectVisitorOs({ userAgent: 'SomeBrowser/1.0', platform: 'Win32' })).toBe('windows')
    expect(detectVisitorOs({ userAgent: 'SomeBrowser/1.0', platform: 'Linux x86_64' })).toBe('linux')
    expect(detectVisitorOs({ userAgent: 'SomeBrowser/1.0', platform: 'MacIntel' })).toBe('macos')
    expect(detectVisitorOs({})).toBeNull()
  })

  test('phones, tablets, and Chromebooks get no desktop OS, though their user agents say Linux or Mac', () => {
    expect(detectVisitorOs(navigatorOf(visitors.android))).toBeNull()
    expect(detectVisitorOs({ userAgent: visitors.android.userAgent, platform: 'Linux armv8l' })).toBeNull()
    expect(
      detectVisitorOs({
        userAgent: 'Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) Chrome/140.0.0.0',
        platform: 'Linux x86_64',
      }),
    ).toBeNull()
    expect(detectVisitorOs({ userAgentData: { platform: 'Chrome OS', mobile: false } })).toBeNull()
    expect(
      detectVisitorOs({
        userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) Mobile/15E148',
        platform: 'iPhone',
      }),
    ).toBeNull()
    // iPadOS Safari asks for the desktop site with a Mac user agent; its touch points give it away.
    expect(
      detectVisitorOs({
        userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Version/26.0 Safari/605.1.15',
        platform: 'MacIntel',
        maxTouchPoints: 5,
      }),
    ).toBeNull()
  })

  test('reads the arch a user agent names', () => {
    expect(detectVisitorArch({ userAgent: visitors.linuxArm.userAgent })).toBe('aarch64')
    expect(detectVisitorArch({ platform: 'Linux aarch64' })).toBe('aarch64')
    expect(detectVisitorArch({ userAgent: visitors.windows.userAgent })).toBe('x86_64')
    expect(detectVisitorArch({ userAgent: visitors.linux.userAgent })).toBe('x86_64')
    expect(detectVisitorArch({ userAgent: 'SomeBrowser/1.0' })).toBeNull()
  })
})

test.describe('download platforms: what a button offers', () => {
  const byOs = (offered: ReturnType<typeof offeredPlatforms>) => Object.fromEntries(offered.map((p) => [p.os, p]))
  const macOnly = byOs(offeredPlatforms(everyOsManifest, oses('macos'), fileUrl))
  const every = byOs(offeredPlatforms(everyOsManifest, oses('macos', 'windows', 'linux'), fileUrl))

  test("a visitor whose OS isn't offered gets the universal Mac build", () => {
    expect(chooseDownload(macOnly, { visitorOs: 'windows', arch: 'x86_64', pinnedOs: null })).toMatchObject({
      os: 'macos',
      option: { arch: 'universal', kind: 'dmg' },
    })
    expect(chooseDownload(macOnly, { visitorOs: null, arch: null, pinnedOs: null })?.option.arch).toBe('universal')
  })

  test("a visitor whose OS is offered gets their OS, and their arch when there's a build for it", () => {
    expect(chooseDownload(every, { visitorOs: 'windows', arch: 'x86_64', pinnedOs: null })?.option.kind).toBe('nsis')
    // Windows on ARM runs the x64 build.
    expect(chooseDownload(every, { visitorOs: 'windows', arch: 'aarch64', pinnedOs: null })?.option.arch).toBe('x86_64')
    // The main link offers the AppImage, never the .deb.
    expect(chooseDownload(every, { visitorOs: 'linux', arch: 'aarch64', pinnedOs: null })?.option).toMatchObject({
      arch: 'aarch64',
      kind: 'appimage',
    })
    expect(chooseDownload(every, { visitorOs: 'macos', arch: 'aarch64', pinnedOs: null })?.option.arch).toBe('aarch64')
  })

  test('a pinned button keeps its OS', () => {
    expect(chooseDownload(every, { visitorOs: 'windows', arch: 'x86_64', pinnedOs: 'linux' })?.os).toBe('linux')
    expect(chooseDownload(macOnly, { visitorOs: 'macos', arch: null, pinnedOs: 'windows' })).toBeNull()
  })

  test('each browser helper is self-contained, since the page inlines its source', () => {
    const rebuilt = <T>(fn: T): T => (new Function(`return (${String(fn)})`) as () => T)()
    expect(rebuilt(detectVisitorOs)(navigatorOf(visitors.windows))).toBe('windows')
    expect(rebuilt(detectVisitorArch)({ platform: 'Linux aarch64' })).toBe('aarch64')
    expect(rebuilt(chooseDownload)(every, { visitorOs: 'linux', arch: 'x86_64', pinnedOs: null })?.option.kind).toBe(
      'appimage',
    )
  })
})

// The built site.

/** Every download href on the page, main links and dropdown options alike. */
async function downloadHrefs(page: Page): Promise<string[]> {
  return page
    .locator('a[data-download-link], [data-download-dropdown] a[data-arch]')
    .evaluateAll((links) => links.map((a) => (a as HTMLAnchorElement).href))
}

test.describe('download section, as published (macOS only)', () => {
  // The built home page offers what the committed config makes public; once another OS is, the fixture
  // describe below covers the shipped states and this one stands down.
  const publicOses = parsePublicDownloadOses(
    readFileSync(new URL('../../api-server/wrangler.toml', import.meta.url), 'utf-8'),
  )
  test.skip(publicOses.size > 1, 'another OS is public, so the home page is no longer macOS-only')

  for (const [name, visitor] of Object.entries(visitors)) {
    test(`a ${name} visitor gets only macOS downloads`, async ({ page }) => {
      await emulateVisitor(page, visitor)
      await page.goto('/')
      await expect(page.locator('html')).toHaveAttribute('data-visitor-os', /.+/)
      const hrefs = await downloadHrefs(page)
      expect(hrefs.length).toBeGreaterThan(0)
      for (const href of hrefs) expect(href).toMatch(/\.dmg$|\/download\/[^/]+\/(aarch64|x86_64|universal)\?/)
      await expect(page.locator('[data-platform-card]:visible')).toHaveCount(1)
      await expect(page.locator('[data-platform-card="macos"]')).toBeVisible()
      await expect(page.locator('[data-show-platform]')).toHaveCount(0)
      await expect(page.locator('.split-btn--hero [data-download-label]')).toHaveText('Download for macOS')
    })
  }

  test('a Windows visitor hears their build is on its way', async ({ page }) => {
    await emulateVisitor(page, visitors.windows)
    await page.goto('/')
    await expect(page.locator('[data-visitor-notice="windows"]')).toBeVisible()
    await expect(page.locator('[data-visitor-notice="windows"]')).toContainText('Cmdr for Windows is on its way')
    await expect(page.locator('[data-visitor-notice="linux"]')).toBeHidden()
    await expect(page.locator('#download .newsletter-inline__label')).toHaveText(
      'Windows coming soon. Linux in alpha (self-build for now). Get notified:',
    )
  })

  test('a Linux visitor hears about the alpha and the build-from-source path', async ({ page }) => {
    await emulateVisitor(page, visitors.linux)
    await page.goto('/')
    const notice = page.locator('[data-visitor-notice="linux"]')
    await expect(notice).toBeVisible()
    await expect(notice.getByRole('link', { name: 'build it from source' })).toHaveAttribute('href', /CONTRIBUTING\.md/)
    await expect(page.locator('[data-visitor-notice="windows"]')).toBeHidden()
  })

  for (const name of ['macArm', 'android'] as const) {
    test(`a ${name} visitor sees no not-yet note`, async ({ page }) => {
      await emulateVisitor(page, visitors[name])
      await page.goto('/')
      await expect(page.locator('html')).toHaveAttribute('data-visitor-os', /.+/)
      await expect(page.locator('[data-visitor-notice]:visible')).toHaveCount(0)
    })
  }

  test('a Mac visitor on Apple Silicon gets the Apple Silicon build, marked', async ({ page }) => {
    await emulateVisitor(page, visitors.macArm)
    await page.goto('/')
    await expect(page.locator('.split-btn--hero a[data-download-link]')).toHaveAttribute('href', /aarch64/)
    await expect(page.locator('.split-btn--hero [data-arch="aarch64"]')).toHaveAttribute('data-recommended', 'true')
  })
})

test.describe('download section, once every OS ships (/download-fixture)', () => {
  test('a Windows visitor gets the Windows installer and card, and can switch to macOS', async ({ page }) => {
    await emulateVisitor(page, visitors.windows)
    await page.goto('/download-fixture')
    const hero = page.locator('[data-fixture-hero] .split-btn')
    await expect(hero.locator('[data-download-label]')).toHaveText('Download for Windows')
    await expect(hero.locator('a[data-download-link]')).toHaveAttribute('href', /Cmdr_9\.9\.0_x64-setup\.exe$/)
    await expect(hero.locator('[data-os-icon="windows"]')).toBeVisible()
    await expect(hero.locator('[data-os-icon="macos"]')).toBeHidden()

    const windowsCard = page.locator('[data-platform-card="windows"]')
    await expect(windowsCard).toBeVisible()
    await expect(windowsCard).toContainText('Run anyway')
    await expect(page.locator('[data-platform-card="macos"]')).toBeHidden()
    await expect(page.locator('[data-visitor-notice]')).toHaveCount(0)

    await windowsCard.getByRole('button', { name: 'macOS' }).click()
    await expect(page.locator('[data-platform-card="macos"]')).toBeVisible()
    await expect(windowsCard).toBeHidden()
  })

  test('a Linux visitor on ARM gets the ARM AppImage, and the dropdown lists every OS', async ({ page }) => {
    await emulateVisitor(page, visitors.linuxArm)
    await page.goto('/download-fixture')
    const hero = page.locator('[data-fixture-hero] .split-btn')
    await expect(hero.locator('a[data-download-link]')).toHaveAttribute('href', /Cmdr_9\.9\.0_aarch64\.AppImage$/)
    await expect(hero.locator('[data-download-label]')).toHaveText('Download for Linux')
    await expect(hero.locator('[data-kind="appimage"][data-arch="aarch64"]')).toHaveAttribute(
      'data-recommended',
      'true',
    )
    await expect(hero.locator('[data-kind="deb"][data-arch="aarch64"]')).not.toHaveAttribute('data-recommended', /.*/)
    await expect(hero.locator('.split-btn__group')).toHaveText(['macOS', 'Windows', 'Linux'])
    await expect(page.locator('[data-platform-card="linux"]')).toBeVisible()
  })

  test('a phone visitor gets macOS, with every OS one tap away', async ({ page }) => {
    await emulateVisitor(page, visitors.android)
    await page.goto('/download-fixture')
    await expect(page.locator('html')).toHaveAttribute('data-visitor-os', 'other')
    const macCard = page.locator('[data-platform-card="macos"]')
    await expect(macCard).toBeVisible()
    await expect(macCard.locator('[data-show-platform]')).toHaveText(['Windows', 'Linux'])
  })
})
