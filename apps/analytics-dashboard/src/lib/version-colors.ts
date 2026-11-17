/**
 * Client-safe: one fixed color per app version, so a version reads the same in every chart and legend,
 * load after load. Color follows the version, never its rank (as `country-colors.ts`): it's a pure function
 * of the version string, so versions leaving the view never recolor the ones that stay. Gold (the product)
 * and Homebrew blue are never a version.
 *
 * A version takes a slot in an eight-color cycle (`versionPalette`):
 * - A release `M.m.0` sits at `(M * 1000 + m) mod 8`, so consecutive minors are cycle neighbors.
 * - A patch `M.m.p` sits at `(M * 1000 + m + 3 + p) mod 8`: four slots from its minor, three before the
 *   next minor, and one after its previous patch. So in stack order (oldest first), neighbors are always
 *   1, 3, or 4 slots apart, and the palette is validated for exactly those distances.
 *
 * Why a validated cycle and not a golden-angle hue walk: the walk failed the dataviz validator in most
 * 20-version windows (cyan hues clip below the chroma floor at a dark-mode lightness, and alternate steps
 * collapse under deuteranopia), and a run-time walk can't be re-validated per window.
 *
 * Validation (dataviz skill's `validate_palette.js --mode dark --surface #1a1917`, 2026-11-17): the cycle in
 * slot order (wrapping) passes with worst adjacent CVD ΔE 11.4 and normal-vision ΔE 16.5, all eight ≥ 3:1 on the
 * surface, lightness and chroma in band; 0.42.0–0.61.0 in stack order (the last 20 releases, patches
 * 0.45.1 and 0.46.1 included) passes too. Picked by a search over an OKLCH grid for: cycle neighbors CVD
 * ΔE ≥ 8 and normal ΔE ≥ 15, the three- and four-slot pairs CVD ΔE ≥ 6 and normal ΔE ≥ 15, any two slots
 * normal ΔE ≥ 12, and every slot ≥ 15 ΔE from gold, Homebrew blue, and the unknown grey.
 * `version-colors.test.ts` re-checks those floors.
 */

/** The cycle, in slot order. ❌ Never reorder or swap one: every version's color would move. */
export const versionPalette = [
  '#6e53c5',
  '#b0451a',
  '#b33492',
  '#ec515d',
  '#c566db',
  '#b78c3c',
  '#2ba996',
  '#258101',
] as const

/** How far a patch's slot sits past its minor's, before its patch number is added. */
const patchSlotOffset = 3

function numericPart(part: string | undefined): number {
  return part !== undefined && /^\d+$/.test(part) ? Number(part) : 0
}

/**
 * A version's slot in `versionPalette`. Parts that aren't plain numbers (a pre-release suffix like
 * `0-beta`) read as zero, so `0.62.0-beta.1` shares `0.62.0`'s color.
 */
export function versionSlot(version: string): number {
  const [major, minor, patch] = version.replace(/^v/, '').split('.').map(numericPart)
  const base = major * 1000 + minor
  const position = patch === 0 ? base : base + patchSlotOffset + patch
  return position % versionPalette.length
}

/** The color for an app version like `0.61.0` (a leading `v` is ignored). */
export function versionColor(version: string): string {
  return versionPalette[versionSlot(version)]
}
