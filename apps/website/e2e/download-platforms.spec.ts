import { readFileSync } from 'node:fs'
import { test, expect, type Page } from '@playwright/test'
import { liveManifestProblem } from '../scripts/live-manifest-check'
import {
  chooseDownload,
  detectVisitorArch,
  detectVisitorOs,
  formatOsList,
  macArchFromRenderer,
  newsletterLabel,
  offeredPlatforms,
  otherDeviceNote,
  parsePublicDownloadOses,
  type DownloadOption,
  type Os,
  type PlatformDownloads,
  type VisitorNavigator,
} from '../src/lib/download-platforms'
import type { UpdateManifest } from '../src/lib/release-downloads'
import { emulateVisitor, visitors, type Visitor } from './visitor'

// The download section and buttons per visitor OS. The first half tests the pure rules in
// `src/lib/download-platforms.ts` and the live-manifest check (no page). The second checks the built site: the home
// page against what the committed config and `public/latest.json` make public (whatever that is, so it keeps
// testing once Windows ships), no-JS defaults, and `/download-fixture`, a release that ships every OS.

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
const withDownloads = (downloads: unknown): UpdateManifest => ({ ...everyOsManifest, downloads }) as UpdateManifest

test.describe('download platforms: which OSes the site offers', () => {
  test('reads PUBLIC_DOWNLOAD_OSES the way the API server does', () => {
    const toml = (value: string) => `[vars]\nPRODUCT_NAME = "Cmdr"\nPUBLIC_DOWNLOAD_OSES = "${value}"\n`
    expect([...parsePublicDownloadOses(toml('macos'))]).toEqual(['macos'])
    expect([...parsePublicDownloadOses(toml('macos,windows'))]).toEqual(['macos', 'windows'])
    expect([...parsePublicDownloadOses(toml(' windows , linux '))]).toEqual(['macos', 'windows', 'linux'])
    expect([...parsePublicDownloadOses(`${toml('macos,windows').trimEnd()} # on since 1.0\n`)]).toEqual([
      'macos',
      'windows',
    ])
    // macOS is always public, whatever the var says, and an unknown OS is ignored.
    expect([...parsePublicDownloadOses(toml(''))]).toEqual(['macos'])
    expect([...parsePublicDownloadOses(toml('solaris'))]).toEqual(['macos'])
  })

  test('refuses a PUBLIC_DOWNLOAD_OSES the site could misread, rather than silently offering macOS only', () => {
    expect(() => parsePublicDownloadOses('[vars]\nPRODUCT_NAME = "Cmdr"\n')).toThrow(/PUBLIC_DOWNLOAD_OSES/)
    expect(() => parsePublicDownloadOses('# PUBLIC_DOWNLOAD_OSES = "macos,windows"\n')).toThrow(/PUBLIC_DOWNLOAD_OSES/)
    // Valid TOML the API would read as Windows on.
    expect(() => parsePublicDownloadOses("PUBLIC_DOWNLOAD_OSES = 'macos,windows'\n")).toThrow(/PUBLIC_DOWNLOAD_OSES/)
    expect(() => parsePublicDownloadOses('PUBLIC_DOWNLOAD_OSES = "macos"\nPUBLIC_DOWNLOAD_OSES = "windows"\n')).toThrow(
      /PUBLIC_DOWNLOAD_OSES/,
    )
  })

  test('the committed wrangler.toml parses', () => {
    const toml = readFileSync(new URL('../../api-server/wrangler.toml', import.meta.url), 'utf-8')
    expect(parsePublicDownloadOses(toml).has('macos')).toBe(true)
  })

  test('a release carrying Windows files offers only macOS until Windows is public', () => {
    const offered = offeredPlatforms(everyOsManifest, oses('macos'), fileUrl)
    expect(offered.map((p) => p.os)).toEqual(['macos'])
  })

  test('a public OS the release carries no file of stays off', () => {
    const macOnly = withDownloads(everyOsManifest.downloads?.filter((d) => d.os === 'macos'))
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
    expect(linux.options.map((o) => o.label)).toEqual(['AppImage · ARM64', 'AppImage · x64', '.deb · ARM64'])
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

  // The review's finding 1: `downloads: []` built a site whose buttons had no link. Finding 2: no universal DMG made
  // every no-JS button the Apple silicon one. Both now stop the build (`release.ts` and the blog plugin call this).
  test('the build stops on a release without the universal macOS DMG, or with a malformed list', () => {
    const noUniversal = withDownloads(everyOsManifest.downloads?.filter((d) => d.arch !== 'universal'))
    for (const manifest of [withDownloads([]), noUniversal]) {
      expect(() => offeredPlatforms(manifest, oses('macos'), fileUrl)).toThrow(/no universal macOS DMG/)
    }
    for (const bad of [{}, [{ os: 'macos' }], [{ ...everyOsManifest.downloads?.[2], size: '91' }]]) {
      expect(() => offeredPlatforms(withDownloads(bad), oses('macos'), fileUrl)).toThrow(/malformed/)
    }
  })

  test('the Docker build refuses the same live manifests before it builds', () => {
    const live = {
      version: '1.2.0',
      notes: 'Notes',
      pub_date: '2026-11-19T00:00:00Z',
      platforms: Object.fromEntries(
        ['darwin-aarch64', 'darwin-x86_64', 'darwin-universal'].map((k) => [k, { url: 'u', signature: 's' }]),
      ),
      downloads: everyOsManifest.downloads,
    }
    expect(liveManifestProblem(live)).toBeNull()
    expect(liveManifestProblem({ ...live, downloads: [] })).toMatch(/universal macOS DMG/)
    expect(
      liveManifestProblem({ ...live, downloads: everyOsManifest.downloads?.filter((d) => d.arch !== 'universal') }),
    ).toMatch(/universal macOS DMG/)
    expect(liveManifestProblem({ ...live, downloads: [{ os: 'macos' }] })).toMatch(/malformed/)
  })

  test('copy follows the offered OSes', () => {
    expect(formatOsList(['macos'])).toBe('macOS')
    expect(formatOsList(['macos', 'windows'])).toBe('macOS and Windows')
    expect(formatOsList(['macos', 'windows', 'linux'])).toBe('macOS, Windows, and Linux')
    expect(newsletterLabel(['macos'])).toBe(
      'Windows is on its way, and Linux is in alpha (self-build for now). The newsletter, about once a month, will announce both:',
    )
    expect(newsletterLabel(['macos', 'windows'])).toBe(
      'Linux is in alpha (self-build for now). The newsletter, about once a month, will announce its download:',
    )
    expect(newsletterLabel(['macos', 'linux'])).toBe(
      'Windows is on its way. The newsletter, about once a month, will announce it:',
    )
    expect(newsletterLabel(['macos', 'windows', 'linux'])).toBe(
      'New versions go out in the newsletter, about once a month:',
    )
    // The newsletter is monthly, so nothing promises a mail on release day.
    for (const offered of [['macos'], ['macos', 'windows']] as Os[][]) {
      expect(newsletterLabel(offered)).not.toMatch(/the day|coming soon/i)
    }
    expect(otherDeviceNote(['macos'])).toBe('Cmdr is a desktop app for macOS.')
    expect(otherDeviceNote(['macos', 'windows'])).toBe('Cmdr is a desktop app for macOS and Windows. Pick yours below.')
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
    expect(detectVisitorOs(navigatorOf(visitors.macSafariIntel))).toBe('macos')
    expect(detectVisitorOs(navigatorOf(visitors.windows))).toBe('windows')
    expect(detectVisitorOs(navigatorOf(visitors.linux))).toBe('linux')
    expect(detectVisitorOs(navigatorOf(visitors.linuxArm))).toBe('linux')
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
    // An Android tablet asking for the desktop site sends a desktop Linux user agent, but on a touch screen.
    expect(detectVisitorOs(navigatorOf(visitors.androidTabletDesktopMode))).toBeNull()
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
    expect(detectVisitorArch({ platform: 'Linux armv8' })).toBe('aarch64')
    // A 32-bit ARM userland: no build of Cmdr fits it.
    expect(detectVisitorArch({ platform: 'Linux armv8l' })).toBeNull()
    expect(detectVisitorArch({ userAgent: visitors.windows.userAgent })).toBe('x86_64')
    expect(detectVisitorArch({ userAgent: visitors.linux.userAgent })).toBe('x86_64')
    expect(detectVisitorArch({ userAgent: 'SomeBrowser/1.0' })).toBeNull()
  })

  test("reads a Mac's arch from the WebGL renderer, and Safari's masked one as unknown", () => {
    expect(macArchFromRenderer(visitors.macArm.webglRenderer)).toBe('aarch64')
    expect(macArchFromRenderer('Apple M3 Max')).toBe('aarch64')
    expect(macArchFromRenderer(visitors.macIntel.webglRenderer)).toBe('x86_64')
    expect(macArchFromRenderer('AMD Radeon Pro 5500M OpenGL Engine')).toBe('x86_64')
    // Safari says "Apple GPU" on every Mac, Intel ones included (the review's finding 12).
    expect(macArchFromRenderer('Apple GPU')).toBeNull()
    expect(macArchFromRenderer('')).toBeNull()
  })
})

test.describe('download platforms: what a button offers', () => {
  const byOs = (offered: PlatformDownloads[]) => Object.fromEntries(offered.map((p) => [p.os, p]))
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
    expect(rebuilt(macArchFromRenderer)('Apple M2')).toBe('aarch64')
    expect(rebuilt(chooseDownload)(every, { visitorOs: 'linux', arch: 'x86_64', pinnedOs: null })?.option.kind).toBe(
      'appimage',
    )
  })
})

// The built site.

const builtManifest = JSON.parse(
  readFileSync(new URL('../public/latest.json', import.meta.url), 'utf-8'),
) as UpdateManifest
const builtPublicOses = parsePublicDownloadOses(
  readFileSync(new URL('../../api-server/wrangler.toml', import.meta.url), 'utf-8'),
)
/** What the build offers, from the same inputs it read; each option's `url` here is its file name. */
const built = offeredPlatforms(builtManifest, builtPublicOses, (d) => d.file)
const builtByOs = Object.fromEntries(built.map((p) => [p.os, p]))
const builtOses = built.map((p) => p.os)

/**
 * Whether `href` downloads `option` of `os`: the mirror's file (no download base, as in the E2E build) or the API
 * server's route for it (`release.ts`).
 */
function isLinkTo(href: string, os: string, option: DownloadOption): boolean {
  const v = builtManifest.version
  if (href.endsWith(`/${option.url}`)) return true
  if (os === 'macos' && option.kind === 'dmg') return href.includes(`/download/${v}/${option.arch}?`)
  return href.includes(`/download/${v}/${os}/${option.arch}?kind=${option.kind}`)
}

/**
 * Who each visitor is, written out by hand so the page check doesn't reuse the code it checks: their OS (null for a
 * device Cmdr doesn't run on) and the arch their own OS's build should match (null: unknown).
 */
const truth: Record<keyof typeof visitors, { os: Os | null; arch: string | null }> = {
  macArm: { os: 'macos', arch: 'aarch64' },
  macIntel: { os: 'macos', arch: 'x86_64' },
  macSafariIntel: { os: 'macos', arch: null },
  windows: { os: 'windows', arch: 'x86_64' },
  linux: { os: 'linux', arch: 'x86_64' },
  linuxArm: { os: 'linux', arch: 'aarch64' },
  android: { os: null, arch: null },
  androidTabletDesktopMode: { os: null, arch: null },
}

/**
 * The file a visitor's auto-picked buttons must end on: their OS's build for their arch when the site offers it,
 * else the universal DMG, which runs on every Mac. Never another OS's arch: a Windows PC's x64 says nothing about
 * which Mac build to offer.
 */
function expectedChoice(name: keyof typeof visitors): { visitorOs: Os | null; os: Os; option: DownloadOption } {
  const { os, arch } = truth[name]
  const own = os ? builtByOs[os] : undefined
  if (own) {
    const main = own.options.filter((o) => o.kind === own.options[0].kind)
    const option = main.find((o) => o.arch === arch) ?? main.find((o) => o.arch === own.defaultArch) ?? main[0]
    return { visitorOs: os, os: own.os, option }
  }
  const universal = builtByOs.macos.options.find((o) => o.kind === 'dmg' && o.arch === 'universal')
  if (!universal) throw new Error('the build has no universal DMG')
  return { visitorOs: os, os: 'macos', option: universal }
}

/** Every download href on the page, main links and dropdown options alike. */
async function downloadHrefs(page: Page): Promise<string[]> {
  return page
    .locator('a[data-download-link], [data-download-dropdown] a[data-arch]')
    .evaluateAll((links) => links.map((a) => (a as HTMLAnchorElement).href))
}

async function heroHref(page: Page): Promise<string> {
  return (await page.locator('.split-btn--hero a[data-download-link]').getAttribute('href')) ?? ''
}

test.describe(`download section, as built (offering ${formatOsList(builtOses)})`, () => {
  for (const [name, visitor] of Object.entries(visitors) as [keyof typeof visitors, Visitor][]) {
    test(`a ${name} visitor gets their build, or the universal DMG, and only offered OSes' files`, async ({ page }) => {
      const { visitorOs, ...choice } = expectedChoice(name)
      await emulateVisitor(page, visitor)
      await page.goto('/')
      await expect(page.locator('html')).toHaveAttribute('data-visitor-os', visitorOs ?? 'other')
      // The old bug: a Windows PC's Intel GPU or a Safari Mac's "Apple GPU" picked a Mac arch. Polled, since Client
      // Hints answer asynchronously.
      await expect.poll(() => heroHref(page).then((h) => isLinkTo(h, choice.os, choice.option))).toBe(true)
      const label = `Download for ${builtByOs[choice.os].name}`
      await expect(page.locator('.split-btn--hero [data-download-label]')).toHaveText(label)

      const hrefs = await downloadHrefs(page)
      expect(hrefs.length).toBeGreaterThan(0)
      for (const href of hrefs) {
        expect(
          built.some((p) => p.options.some((o) => isLinkTo(href, p.os, o))),
          href,
        ).toBe(true)
      }

      // The ✓ marks the visitor's own build only.
      const recommended = page.locator('.split-btn--hero [data-recommended]')
      if (choice.os === visitorOs && choice.option.arch !== builtByOs[choice.os].defaultArch) {
        await expect(recommended).toHaveAttribute('data-arch', choice.option.arch)
      } else if (choice.os !== visitorOs) {
        await expect(recommended).toHaveCount(0)
      }

      const shownCard = choice.os
      await expect(page.locator('[data-platform-card]:visible')).toHaveCount(1)
      await expect(page.locator(`[data-platform-card="${shownCard}"]`)).toBeVisible()

      // A note only for a visitor whose OS isn't offered, or that detection can't place.
      const notice = page.locator('[data-visitor-notice]:visible')
      if (visitorOs !== null && builtOses.includes(visitorOs)) await expect(notice).toHaveCount(0)
      else await expect(notice).toHaveAttribute('data-visitor-notice', visitorOs ?? 'other')

      await expect(page.locator('#download .newsletter-inline__label')).toHaveText(newsletterLabel(builtOses))
    })
  }

  test('a Linux visitor, while Linux is out of reach, gets the build-from-source path', async ({ page }) => {
    test.skip(builtOses.includes('linux'), 'Linux is offered, so its visitors get the download')
    await emulateVisitor(page, visitors.linux)
    await page.goto('/')
    const notice = page.locator('[data-visitor-notice="linux"]')
    await expect(notice).toBeVisible()
    await expect(notice.getByRole('link', { name: 'build it from source' })).toHaveAttribute('href', /CONTRIBUTING\.md/)
  })

  test('an Android tablet in desktop-site mode gets the neutral note, never build-from-source', async ({ page }) => {
    await emulateVisitor(page, visitors.androidTabletDesktopMode)
    await page.goto('/')
    await expect(page.locator('[data-visitor-notice="other"]')).toHaveText(otherDeviceNote(builtOses))
    await expect(page.locator('[data-visitor-notice="linux"]')).toBeHidden()
  })
})

test.describe('download buttons with JS off', () => {
  test.use({ javaScriptEnabled: false })

  // Without the picker every button keeps its server-rendered link, which must run on every Mac: an Intel Mac must
  // never get the Apple silicon DMG (the review's finding 2).
  for (const path of ['/', '/pricing/', '/blog/total-commander-for-macos/']) {
    test(`${path} offers the universal DMG on every main link`, async ({ page }) => {
      await emulateVisitor(page, visitors.macIntel)
      await page.goto(path)
      const universal = builtByOs.macos.options.find((o) => o.arch === 'universal')
      if (!universal) throw new Error('the build has no universal DMG')
      const mains = await page
        .locator('a[data-download-link]')
        .evaluateAll((links) => links.map((a) => (a as HTMLAnchorElement).href))
      for (const href of mains) expect(isLinkTo(href, 'macos', universal), href).toBe(true)
      if (path !== '/blog/total-commander-for-macos/') expect(mains.length).toBeGreaterThan(0)
    })
  }
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
    await expect(page.locator('[data-visitor-notice]:visible')).toHaveCount(0)

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

  test('a phone visitor gets macOS and a neutral note, with every OS one tap away', async ({ page }) => {
    await emulateVisitor(page, visitors.android)
    await page.goto('/download-fixture')
    await expect(page.locator('html')).toHaveAttribute('data-visitor-os', 'other')
    await expect(page.locator('[data-visitor-notice="other"]')).toHaveText(
      'Cmdr is a desktop app for macOS, Windows, and Linux. Pick yours below.',
    )
    const macCard = page.locator('[data-platform-card="macos"]')
    await expect(macCard).toBeVisible()
    await expect(macCard.locator('[data-show-platform]')).toHaveText(['Windows', 'Linux'])
  })
})
