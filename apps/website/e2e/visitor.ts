import type { Page } from '@playwright/test'

/**
 * A visitor's OS and CPU, as the download picker reads them (`detectVisitorOs`, `detectVisitorArch`, and
 * `macArchFromRenderer` in `src/lib/download-platforms.ts`). Playwright's `userAgent` option changes neither
 * `navigator.platform` nor `navigator.userAgentData`, and the WebGL renderer is the host's GPU, so a spec sets all of
 * them here or it tests the machine it runs on.
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
  /** What WebGL's `UNMASKED_RENDERER_WEBGL` answers. */
  webglRenderer: string
}

const chrome = 'AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'
const macUa = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)'
const intelGpu = 'ANGLE (Intel, Intel(R) UHD Graphics 630 (0x00003E9B) Direct3D11 vs_5_0 ps_5_0, D3D11)'

export const visitors = {
  macArm: {
    userAgent: `${macUa} ${chrome}`,
    platform: 'MacIntel',
    hintsPlatform: 'macOS',
    architecture: 'arm',
    webglRenderer: 'ANGLE (Apple, ANGLE Metal Renderer: Apple M2, Unspecified Version)',
  },
  macIntel: {
    userAgent: `${macUa} ${chrome}`,
    platform: 'MacIntel',
    hintsPlatform: 'macOS',
    architecture: 'x86',
    webglRenderer: 'ANGLE (Intel Inc., Intel(R) Iris(TM) Plus Graphics 655, OpenGL 4.1)',
  },
  // Safari on an Intel Mac: no Client Hints, and a renderer string that names no GPU.
  macSafariIntel: {
    userAgent: `${macUa} AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15`,
    platform: 'MacIntel',
    hintsPlatform: null,
    webglRenderer: 'Apple GPU',
  },
  // An Intel GPU, so a picker that reads a Mac's arch from WebGL for any visitor would offer the Intel DMG.
  windows: {
    userAgent: `Mozilla/5.0 (Windows NT 10.0; Win64; x64) ${chrome}`,
    platform: 'Win32',
    hintsPlatform: 'Windows',
    architecture: 'x86',
    webglRenderer: intelGpu,
  },
  linux: {
    userAgent: 'Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0',
    platform: 'Linux x86_64',
    hintsPlatform: null,
    webglRenderer: 'Mesa Intel(R) UHD Graphics 620 (KBL GT2)',
  },
  linuxArm: {
    userAgent: `Mozilla/5.0 (X11; Linux aarch64) ${chrome}`,
    platform: 'Linux aarch64',
    hintsPlatform: 'Linux',
    architecture: 'arm',
    webglRenderer: 'Mali-G610',
  },
  android: {
    userAgent: `Mozilla/5.0 (Linux; Android 14; Pixel 9) ${chrome.replace('Safari', 'Mobile Safari')}`,
    platform: 'Linux armv8l',
    hintsPlatform: 'Android',
    mobile: true,
    maxTouchPoints: 5,
    architecture: 'arm',
    webglRenderer: 'Adreno (TM) 740',
  },
  // Firefox for Android asking for the desktop site: a desktop Linux user agent on a touch screen.
  androidTabletDesktopMode: {
    userAgent: 'Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0',
    platform: 'Linux armv8l',
    hintsPlatform: null,
    maxTouchPoints: 5,
    webglRenderer: 'Adreno (TM) 740',
  },
} satisfies Record<string, Visitor>

/** Make the page's `navigator` and WebGL report `visitor`, from before any page script runs. */
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
    // UNMASKED_RENDERER_WEBGL, from the WEBGL_debug_renderer_info extension.
    const unmaskedRenderer = 0x9246
    // Headless Chromium may not expose the extension at all, so it's provided too.
    const getExtension = WebGLRenderingContext.prototype.getExtension as (
      this: WebGLRenderingContext,
      n: string,
    ) => unknown
    const fakeGetExtension = function (this: WebGLRenderingContext, name: string): unknown {
      return name === 'WEBGL_debug_renderer_info'
        ? { UNMASKED_RENDERER_WEBGL: unmaskedRenderer }
        : getExtension.call(this, name)
    }
    WebGLRenderingContext.prototype.getExtension =
      fakeGetExtension as typeof WebGLRenderingContext.prototype.getExtension
    const getParameter = WebGLRenderingContext.prototype.getParameter
    WebGLRenderingContext.prototype.getParameter = function (this: WebGLRenderingContext, name: number) {
      return name === unmaskedRenderer ? v.webglRenderer : (getParameter.call(this, name) as unknown)
    }
  }, visitor)
}
