import { describe, expect, it } from 'vitest'
import { versionColor, versionPalette, versionSlot } from './version-colors.js'
import { COLOR_GOLD, COLOR_HOMEBREW } from './colors.js'

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

const surface = '#1a1917'

describe('versionColor', () => {
  it('depends only on the version, whatever else is in view', () => {
    const alone = versionColor('0.61.0')
    const amongOthers = ['0.40.0', '0.59.0', '0.60.0', '0.61.0', '0.62.0'].map(versionColor)[3]
    expect(amongOthers).toBe(alone)
    expect(versionColor('v0.61.0')).toBe(alone)
  })

  it('gives consecutive minors neighboring slots', () => {
    expect((versionSlot('0.61.0') - versionSlot('0.60.0') + 8) % 8).toBe(1)
  })

  it('reads a pre-release as its release', () => {
    expect(versionColor('0.62.0-beta.1')).toBe(versionColor('0.62.0'))
  })

  it('is never gold or Homebrew blue', () => {
    for (const color of versionPalette) {
      expect(normalDeltaE(color, COLOR_GOLD)).toBeGreaterThanOrEqual(15)
      expect(normalDeltaE(color, COLOR_HOMEBREW)).toBeGreaterThanOrEqual(15)
    }
  })

  it('keeps every slot at 3:1 or more on the dark surface', () => {
    for (const color of versionPalette) expect(contrast(color, surface)).toBeGreaterThanOrEqual(3)
  })

  // Stack neighbors are 1 (consecutive minors or patches), 3, or 4 slots apart (a patch next to a minor).
  it.each([1, 3, 4])('keeps slots %i apart above the validator floors', (distance) => {
    for (let i = 0; i < versionPalette.length; i++) {
      const a = versionPalette[i]
      const b = versionPalette[(i + distance) % versionPalette.length]
      expect(cvdDeltaE(a, b), `${a} ↔ ${b}`).toBeGreaterThanOrEqual(6)
      expect(normalDeltaE(a, b), `${a} ↔ ${b}`).toBeGreaterThanOrEqual(15)
    }
  })

  it('keeps stack neighbors 1, 3, or 4 slots apart across patches', () => {
    const stack = ['0.44.0', '0.45.0', '0.45.1', '0.45.2', '0.45.3', '0.46.0', '0.46.1', '0.47.0']
    for (let i = 1; i < stack.length; i++) {
      const forward = (versionSlot(stack[i]) - versionSlot(stack[i - 1]) + 8) % 8
      expect([1, 3, 4]).toContain(Math.min(forward, 8 - forward))
    }
  })
})
