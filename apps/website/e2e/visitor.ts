import type { Page } from '@playwright/test'

/**
 * A visitor's OS and CPU, as the download picker reads them (`detectVisitorOs` / `detectVisitorArch` in
 * `src/lib/download-platforms.ts`). Playwright's `userAgent` option changes neither `navigator.platform` nor
 * `navigator.userAgentData`, and the picker trusts Client Hints first, so a spec sets all three here or it tests the
 * machine it runs on.
 */
export interface Visitor {
  userAgent: string
  platform: string
  /** `navigator.userAgentData.platform`; null for a browser without Client Hints (Safari, Firefox). */
  hintsPlatform: string | null
  mobile?: boolean
  maxTouchPoints?: number
  /** What `getHighEntropyValues(['architecture'])` answers: `arm` or `x86`. */
  architecture?: string
}

const chrome = 'AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'

export const visitors = {
  macArm: {
    userAgent: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ${chrome}`,
    platform: 'MacIntel',
    hintsPlatform: 'macOS',
    architecture: 'arm',
  },
  macIntel: {
    userAgent: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ${chrome}`,
    platform: 'MacIntel',
    hintsPlatform: 'macOS',
    architecture: 'x86',
  },
  windows: {
    userAgent: `Mozilla/5.0 (Windows NT 10.0; Win64; x64) ${chrome}`,
    platform: 'Win32',
    hintsPlatform: 'Windows',
    architecture: 'x86',
  },
  linux: {
    userAgent: 'Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0',
    platform: 'Linux x86_64',
    hintsPlatform: null,
  },
  linuxArm: {
    userAgent: `Mozilla/5.0 (X11; Linux aarch64) ${chrome}`,
    platform: 'Linux aarch64',
    hintsPlatform: 'Linux',
    architecture: 'arm',
  },
  android: {
    userAgent: `Mozilla/5.0 (Linux; Android 14; Pixel 9) ${chrome.replace('Safari', 'Mobile Safari')}`,
    platform: 'Linux armv8l',
    hintsPlatform: 'Android',
    mobile: true,
    maxTouchPoints: 5,
    architecture: 'arm',
  },
} satisfies Record<string, Visitor>

/** Make the page's `navigator` report `visitor`, from before any page script runs. */
export async function emulateVisitor(page: Page, visitor: Visitor): Promise<void> {
  await page.addInitScript((v: Visitor) => {
    const proto = Object.getPrototypeOf(navigator) as object
    const define = (name: string, value: unknown) => {
      Object.defineProperty(proto, name, { get: () => value, configurable: true })
    }
    define('userAgent', v.userAgent)
    define('platform', v.platform)
    define('maxTouchPoints', v.maxTouchPoints ?? 0)
    define(
      'userAgentData',
      v.hintsPlatform === null
        ? undefined
        : {
            platform: v.hintsPlatform,
            mobile: v.mobile ?? false,
            getHighEntropyValues: () => Promise.resolve({ architecture: v.architecture ?? '' }),
          },
    )
  }, visitor)
}
