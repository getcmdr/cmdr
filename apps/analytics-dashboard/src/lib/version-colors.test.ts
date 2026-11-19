import { describe, expect, it } from 'vitest'
import { latestCount, latestPalette, olderColor, versionColors } from './version-colors.js'
import { COLOR_GOLD, COLOR_HOMEBREW, COLOR_UNKNOWN } from './colors.js'
import { compareVersions } from './rollout.js'

// The dataviz validator's math (OKLab ΔE ×100, Machado 2009 CVD at severity 1.0, WCAG contrast), so the
// floors it enforces stay pinned here without the skill's script.
const machado = {
  protan: [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
  ],
  deutan: [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.01182, 0.04294, 0.968881],
  ],
}

function linear(hex: string): number[] {
  return [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  })
}

function oklab([r, g, b]: number[]): number[] {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ]
}

function simulate(hex: string, kind: keyof typeof machado): number[] {
  const rgb = linear(hex)
  return machado[kind].map((row) => Math.min(1, Math.max(0, row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2])))
}

function deltaE(a: number[], b: number[]): number {
  const [x, y] = [oklab(a), oklab(b)]
  return 100 * Math.hypot(x[0] - y[0], x[1] - y[1], x[2] - y[2])
}

const normalDeltaE = (a: string, b: string): number => deltaE(linear(a), linear(b))
const cvdDeltaE = (a: string, b: string): number =>
  Math.min(deltaE(simulate(a, 'protan'), simulate(b, 'protan')), deltaE(simulate(a, 'deutan'), simulate(b, 'deutan')))

function contrast(a: string, b: string): number {
  const luminance = (hex: string): number => {
    const [r, g, b2] = linear(hex)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b2
  }
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

function oklch(hex: string): { lightness: number; chroma: number; hue: number } {
  const [lightness, a, b] = oklab(linear(hex))
  return { lightness, chroma: Math.hypot(a, b), hue: ((Math.atan2(b, a) * 180) / Math.PI + 360) % 360 }
}

const surface = '#1a1917'
/** The older ramp, sampled finely enough to catch any point that drifts toward another color. */
const rampSamples = Array.from({ length: 21 }, (_, i) => olderColor(i / 20))
/** 25 versions in view, like the live 30-day chart: 0.37–0.61 plus two patches. */
const liveVersions = [...Array.from({ length: 25 }, (_, i) => `0.${String(37 + i)}.0`), '0.45.1', '0.58.1']

describe('versionColors', () => {
  it('gives the eight newest versions the strong palette, newest first', () => {
    const colors = versionColors(liveVersions)
    expect(colors.get('0.61.0')).toBe(latestPalette[0])
    expect(colors.get('0.58.1')).toBe(latestPalette[3])
    expect(colors.get('0.58.0')).toBe(latestPalette[4])
    expect(colors.get('0.55.0')).toBe(latestPalette[7])
    expect(colors.get('0.54.0')).toBe(olderColor(0))
    expect(colors.get('0.37.0')).toBe(olderColor(1))
  })

  it('gives every version a color, the strong eight all different', () => {
    const colors = versionColors(liveVersions)
    expect(colors.size).toBe(liveVersions.length)
    expect(new Set(latestPalette).size).toBe(latestCount)
  })

  it('is stable for a given list, whatever its order or duplicates', () => {
    const shuffled = [...liveVersions].reverse()
    expect(versionColors([...shuffled, '0.50.0'])).toEqual(versionColors(liveVersions))
  })

  it('shifts every color one step when a release ships', () => {
    const before = versionColors(liveVersions)
    const after = versionColors([...liveVersions, '0.62.0'])
    expect(after.get('0.62.0')).toBe(latestPalette[0])
    expect(after.get('0.61.0')).toBe(latestPalette[1])
    expect(after.get('0.55.0')).toBe(olderColor(0))
    expect(before.get('0.61.0')).toBe(latestPalette[0])
  })

  it('orders by version, so a pre-release sits below its release and v-prefixes are ignored', () => {
    const colors = versionColors(['0.62.0', '0.62.0-beta.1', 'v0.61.0'])
    expect(colors.get('0.62.0')).toBe(latestPalette[0])
    expect(colors.get('0.62.0-beta.1')).toBe(latestPalette[1])
    expect(colors.get('v0.61.0')).toBe(latestPalette[2])
  })

  it('uses only strong colors with eight or fewer versions, and the ramp start for a lone ninth', () => {
    const eight = Array.from({ length: 8 }, (_, i) => `0.${String(50 + i)}.0`)
    const colors = versionColors(eight)
    expect(eight.map((v) => colors.get(v))).toEqual([...latestPalette].reverse())
    expect(versionColors(['0.49.0', ...eight]).get('0.49.0')).toBe(olderColor(0))
    expect(versionColors([])).toEqual(new Map())
  })

  it('fades older versions monotonically with age', () => {
    const colors = versionColors(liveVersions)
    const older = [...liveVersions]
      .sort((a, b) => compareVersions(b, a))
      .slice(latestCount)
      .map((v) => colors.get(v) ?? '')
    expect(older).toHaveLength(liveVersions.length - latestCount)
    for (let i = 1; i < older.length; i++) {
      const [newer, aged] = [oklch(older[i - 1]), oklch(older[i])]
      expect(aged.lightness, `${older[i - 1]} → ${older[i]}`).toBeLessThan(newer.lightness)
      expect(aged.chroma, `${older[i - 1]} → ${older[i]}`).toBeLessThan(newer.chroma)
      expect(contrast(older[i], surface)).toBeLessThan(contrast(older[i - 1], surface))
    }
  })
})

describe('latestPalette', () => {
  it('sits in the validator dark band, above the chroma floor, and at 3:1 on the surface', () => {
    for (const color of latestPalette) {
      const { lightness, chroma } = oklch(color)
      expect(lightness, color).toBeGreaterThanOrEqual(0.48)
      expect(lightness, color).toBeLessThanOrEqual(0.67)
      expect(chroma, color).toBeGreaterThanOrEqual(0.1)
      expect(contrast(color, surface), color).toBeGreaterThanOrEqual(3)
    }
  })

  it('keeps stack neighbors at the validator targets', () => {
    for (let i = 1; i < latestPalette.length; i++) {
      const [a, b] = [latestPalette[i - 1], latestPalette[i]]
      expect(cvdDeltaE(a, b), `${a} ↔ ${b}`).toBeGreaterThanOrEqual(8)
      expect(normalDeltaE(a, b), `${a} ↔ ${b}`).toBeGreaterThanOrEqual(15)
    }
  })

  it('keeps all eight apart from each other, not only neighbors', () => {
    for (let i = 0; i < latestPalette.length; i++) {
      for (let j = i + 1; j < latestPalette.length; j++) {
        const [a, b] = [latestPalette[i], latestPalette[j]]
        expect(normalDeltaE(a, b), `${a} ↔ ${b}`).toBeGreaterThanOrEqual(15)
        expect(cvdDeltaE(a, b), `${a} ↔ ${b}`).toBeGreaterThanOrEqual(7)
      }
    }
  })
})

describe('olderColor', () => {
  it('starts far from the eighth-newest color it stacks under', () => {
    const [eighth, first] = [latestPalette[latestCount - 1], olderColor(0)]
    expect(cvdDeltaE(eighth, first)).toBeGreaterThanOrEqual(8)
    expect(normalDeltaE(eighth, first)).toBeGreaterThanOrEqual(15)
  })

  it('never comes close to a strong color', () => {
    for (const older of rampSamples) {
      for (const strong of latestPalette)
        expect(normalDeltaE(older, strong), `${older} ↔ ${strong}`).toBeGreaterThanOrEqual(10)
    }
  })

  it('keeps its newer two-thirds at 3:1 on the surface', () => {
    for (const color of rampSamples.slice(0, 14)) expect(contrast(color, surface), color).toBeGreaterThanOrEqual(3)
  })

  it('clamps outside 0–1', () => {
    expect(olderColor(-1)).toBe(olderColor(0))
    expect(olderColor(2)).toBe(olderColor(1))
  })
})

describe('every version color', () => {
  it('is never gold, Homebrew blue, or the unknown grey, nor an ochre that reads as gold', () => {
    for (const color of [...latestPalette, ...rampSamples]) {
      expect(normalDeltaE(color, COLOR_GOLD), color).toBeGreaterThanOrEqual(15)
      expect(normalDeltaE(color, COLOR_HOMEBREW), color).toBeGreaterThanOrEqual(15)
      // The muted ramp sits closer to grey than the strong eight, but its tint keeps it apart.
      expect(normalDeltaE(color, COLOR_UNKNOWN), color).toBeGreaterThanOrEqual(7)
      const { hue } = oklch(color)
      expect(hue < 48 || hue > 128, `${color} hue ${hue.toFixed(0)}`).toBe(true)
    }
  })
})
