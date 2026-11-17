import { describe, expect, it } from 'vitest'
import { versionColor, versionPalette, versionSlot } from './version-colors.js'
import { COLOR_GOLD, COLOR_HOMEBREW, COLOR_UNKNOWN } from './colors.js'

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
const minorSlots = 16
const minorColor = (slot: number): string => versionPalette[slot % minorSlots]
const patchColor = (slot: number): string => versionPalette[minorSlots + (slot % minorSlots)]

/** Every pair that can meet in a stack, by the slot rule in the header, as [a, b, why]. */
function stackNeighborPairs(): [string, string, string][] {
  const pairs: [string, string, string][] = []
  for (let i = 0; i < minorSlots; i++) {
    const slot = String(i)
    pairs.push([minorColor(i), minorColor(i + 1), `minor ${slot} → next minor`])
    pairs.push([minorColor(i), patchColor(i), `minor ${slot} → its first patch`])
    pairs.push([patchColor(i), minorColor(i + 1), `patch ${slot} → next minor`])
    pairs.push([patchColor(i), patchColor(i + 8), `patch ${slot} → its second patch`])
    pairs.push([patchColor(i + 8), minorColor(i + 1), `second patch of minor ${slot} → next minor`])
  }
  return pairs
}

describe('versionColor', () => {
  it('depends only on the version, whatever else is in view', () => {
    const alone = versionColor('0.61.0')
    const amongOthers = ['0.40.0', '0.59.0', '0.60.0', '0.61.0', '0.62.0'].map(versionColor)[3]
    expect(amongOthers).toBe(alone)
    expect(versionColor('v0.61.0')).toBe(alone)
  })

  it('gives consecutive minors neighboring minor slots and patches the patch half', () => {
    expect((versionSlot('0.61.0') - versionSlot('0.60.0') + minorSlots) % minorSlots).toBe(1)
    expect(versionSlot('0.61.0')).toBeLessThan(minorSlots)
    expect(versionSlot('0.61.1')).toBe(minorSlots + versionSlot('0.61.0'))
    expect(versionSlot('0.61.2')).toBe(minorSlots + ((versionSlot('0.61.0') + 8) % minorSlots))
  })

  it('reads a pre-release as its release', () => {
    expect(versionColor('0.62.0-beta.1')).toBe(versionColor('0.62.0'))
  })

  it('never repeats a color across the last 16 minors and their patches', () => {
    const minors = Array.from({ length: 16 }, (_, i) => `0.${String(46 + i)}.0`)
    const window = [...minors, '0.46.1', '0.52.1', '0.52.2', '0.55.1', '0.58.1', '0.61.1', '0.61.2']
    const colors = window.map(versionColor)
    expect(new Set(colors).size, colors.join(' ')).toBe(window.length)
  })

  it('has 32 distinct slots, no two near-identical', () => {
    expect(versionPalette).toHaveLength(2 * minorSlots)
    for (let i = 0; i < versionPalette.length; i++) {
      for (let j = i + 1; j < versionPalette.length; j++) {
        const [a, b] = [versionPalette[i], versionPalette[j]]
        expect(normalDeltaE(a, b), `${a} ↔ ${b}`).toBeGreaterThanOrEqual(4.5)
      }
    }
  })

  it('is never gold, Homebrew blue, or the unknown grey', () => {
    for (const color of versionPalette) {
      expect(normalDeltaE(color, COLOR_GOLD), color).toBeGreaterThanOrEqual(15)
      expect(normalDeltaE(color, COLOR_HOMEBREW), color).toBeGreaterThanOrEqual(15)
      expect(normalDeltaE(color, COLOR_UNKNOWN), color).toBeGreaterThanOrEqual(15)
      // No ochre, gold, yellow, or olive: those read as the product.
      const { hue } = oklch(color)
      expect(hue < 48 || hue > 128, `${color} hue ${hue.toFixed(0)}`).toBe(true)
    }
  })

  it('keeps every slot in the validator dark band, above the chroma floor, and at 3:1 on the surface', () => {
    for (const color of versionPalette) {
      const { lightness, chroma } = oklch(color)
      expect(lightness, color).toBeGreaterThanOrEqual(0.48)
      expect(lightness, color).toBeLessThanOrEqual(0.67)
      expect(chroma, color).toBeGreaterThanOrEqual(0.1)
      expect(contrast(color, surface), color).toBeGreaterThanOrEqual(3)
    }
  })

  it('keeps every possible stack neighbor at the validator targets', () => {
    for (const [a, b, why] of stackNeighborPairs()) {
      expect(cvdDeltaE(a, b), `${why}: ${a} ↔ ${b}`).toBeGreaterThanOrEqual(8)
      expect(normalDeltaE(a, b), `${why}: ${a} ↔ ${b}`).toBeGreaterThanOrEqual(15)
    }
  })

  // A minor nobody runs drops out of the stack; its neighbors then touch, at a lower bar.
  it('keeps minors two apart above the CVD floor', () => {
    for (let i = 0; i < minorSlots; i++) {
      const [a, b] = [minorColor(i), minorColor(i + 2)]
      expect(cvdDeltaE(a, b), `${a} ↔ ${b}`).toBeGreaterThanOrEqual(6)
      expect(normalDeltaE(a, b), `${a} ↔ ${b}`).toBeGreaterThanOrEqual(11.5)
    }
  })
})
