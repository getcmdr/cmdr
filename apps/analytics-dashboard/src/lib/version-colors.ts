/**
 * Client-safe: the app versions' colors on the rollout chart. Color follows recency, so the versions that
 * matter stand out: the eight newest versions in view get strong, distinct colors (`latestPalette`, newest
 * first), and every older version gets its own step on one muted ramp that fades with age (`olderColor`).
 * No version folds into a grey "Older" bucket; the legend and tooltip name each one.
 *
 * - A color is a pure function of the version and the set of versions in view, so a given dataset always
 *   draws the same. When a release ships, every version moves one step down (accepted, #1217).
 * - Each distinct version string is its own step, patches and pre-releases included: a patch and its minor
 *   draw as adjacent layers, so sharing a color would merge them, and a patch usually replaces its minor
 *   among the newest installs, so it earns its own strong slot.
 * - Gold (the product), Homebrew blue, and the unknown grey are never a version, and no color sits in the
 *   ochre-to-olive hues (OKLCH 48°–128°) that read as gold.
 *
 * Validation (dataviz skill's `validate_palette.js --mode dark --surface #1a1917`, 2026-11-19):
 * - `latestPalette` in stack order (oldest of the eight at the bottom): PASS, worst adjacent CVD ΔE 11.2
 *   (deutan), worst adjacent normal ΔE 21.5. With `--pairs all`: PASS with a CVD WARN, worst CVD ΔE 7.6
 *   (#3aa593 ↔ #c97d68, not neighbors), worst normal ΔE 16.0. All eight in L 0.48–0.67, chroma ≥ 0.10, ≥ 3:1.
 * - The ramp: OKLCH (0.68, 0.11, 335°) to (0.44, 0.07, 305°), mauve to dusk plum. Its start sits ΔE 21.7
 *   (CVD 17.6) from the eighth color it stacks under; every point stays ≥ ΔE 10.7 from the strong eight,
 *   ≥ 7.3 from the unknown grey, and its newer two-thirds keep 3:1 on the surface (the oldest fade below).
 * Picked by simulated annealing over an OKLCH grid: hue families first, then neighbor and all-pairs floors.
 * `version-colors.test.ts` re-checks those floors.
 */
import { compareVersions } from './rollout.js'

/** The eight newest versions' colors, newest first. Neighbors here are neighbors in the stack. */
export const latestPalette = [
  '#3aa593',
  '#b13b6e',
  '#ac6ef8',
  '#0d8500',
  '#ef10c8',
  '#c97d68',
  '#534bfb',
  '#f50004',
] as const

export const latestCount = latestPalette.length

/** The older ramp's ends in OKLCH: `from` is the newest older version, `to` the oldest. */
const olderRamp = {
  from: { lightness: 0.68, chroma: 0.11, hue: 335 },
  to: { lightness: 0.44, chroma: 0.07, hue: 305 },
}

/** A point on the older ramp: `t` 0 is the newest older version, 1 the oldest (clamped). */
export function olderColor(t: number): string {
  const at = Math.min(1, Math.max(0, t))
  const mix = (key: keyof typeof olderRamp.from): number =>
    olderRamp.from[key] + (olderRamp.to[key] - olderRamp.from[key]) * at
  return oklchToHex(mix('lightness'), mix('chroma'), mix('hue'))
}

/** Every version's color, keyed by the version string as given. Order and duplicates in `versions` don't matter. */
export function versionColors(versions: readonly string[]): Map<string, string> {
  const newestFirst = [...new Set(versions)].sort((a, b) => compareVersions(b.replace(/^v/, ''), a.replace(/^v/, '')))
  const olderSteps = newestFirst.length - latestCount
  return new Map(
    newestFirst.map((version, rank) => {
      if (rank < latestCount) return [version, latestPalette[rank]]
      const step = rank - latestCount
      return [version, olderColor(olderSteps > 1 ? step / (olderSteps - 1) : 0)]
    }),
  )
}

function oklchToHex(lightness: number, chroma: number, hue: number): string {
  const radians = (hue * Math.PI) / 180
  const [a, b] = [chroma * Math.cos(radians), chroma * Math.sin(radians)]
  const l = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (lightness - 0.0894841775 * a - 1.291485548 * b) ** 3
  const linear = [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
  return `#${linear
    .map((c) => {
      const clamped = Math.min(1, Math.max(0, c))
      const srgb = clamped <= 0.0031308 ? 12.92 * clamped : 1.055 * clamped ** (1 / 2.4) - 0.055
      return Math.round(srgb * 255)
        .toString(16)
        .padStart(2, '0')
    })
    .join('')}`
}
