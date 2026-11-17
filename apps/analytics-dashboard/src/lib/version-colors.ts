/**
 * Client-safe: one fixed color per app version, so a version reads the same in every chart and legend,
 * load after load. Color follows the version, never its rank (as `country-colors.ts`): it's a pure function
 * of the version string, so versions leaving the view never recolor the ones that stay. Gold (the product),
 * Homebrew blue, and the unknown grey are never a version, and no slot sits in the ochre-to-olive hues
 * (OKLCH 48°–128°) that read as gold.
 *
 * `versionPalette` has 32 slots: 16 for releases, 16 for patches.
 * - A release `M.m.0` sits at minor slot `i = (M * 1000 + m) mod 16`, so consecutive minors are neighbors and
 *   any 16 consecutive minors (a 90-day window spans about 12) all differ.
 * - Its first patch `M.m.1` sits at `16 + i`, its second at `16 + (i + 8) mod 16`. Patches of different minors
 *   share a color only when their minors are a multiple of 16 apart, or a second patch meets the first patch
 *   of the minor 8 away. A third patch wraps onto the first's color; none has shipped since 0.36.2.
 * - So in stack order (oldest first), the pairs that can touch are: minor i ↔ minor i + 1, minor i ↔ patch i,
 *   patch i ↔ minor i + 1, patch i ↔ patch i + 8, and patch i + 8 ↔ minor i + 1. The palette is validated for
 *   exactly those, plus minor i ↔ minor i + 2 at a lower bar (a minor nobody runs drops out of the stack).
 *
 * Why 16 + 16 and not a plain cycle with patches at an offset: no fixed offset keeps a patch off all 16 minor
 * colors, and a 32-slot interleave (minors on evens) can't meet the neighbor floors in the dark band. Why a
 * validated table and not a golden-angle hue walk: the walk failed the validator in most 20-version windows.
 *
 * Validation (dataviz skill's `validate_palette.js --mode dark --surface #1a1917`, 2026-11-17):
 * - The 16 minor slots in order, wrapping: PASS, worst adjacent CVD ΔE 8.4 (protan), normal ΔE 15.9.
 * - 0.46.0–0.61.0 in stack order with patches 0.46.1, 0.52.1, 0.52.2, 0.55.1, 0.58.1, 0.61.1, 0.61.2: PASS,
 *   worst adjacent CVD ΔE 8.4, normal ΔE 15.9.
 * - Across every neighbor pair above: worst CVD ΔE 8.4, normal ΔE 15.8; minors two apart CVD 6.1, normal 12.0.
 *   All 32 sit in L 0.48–0.67, chroma ≥ 0.10, ≥ 3:1 on the surface, ≥ 18 ΔE from gold, blue, and grey.
 * - All 32 as all-pairs FAILs (closest two ΔE 4.8): 32 dark-band hues can't all be far apart, so identity
 *   for non-neighbors comes from the legend and tooltip, as the rollout chart already does.
 * Picked by simulated annealing over an OKLCH grid (neighbor floors first, then the all-pairs minimum).
 * `version-colors.test.ts` re-checks those floors.
 */

/** Minor slots 0–15, then patch slots 16–31. ❌ Never reorder or swap one: every version's color would move. */
export const versionPalette = [
  // Releases, by `(M * 1000 + m) mod 16`.
  '#c83846',
  '#803de9',
  '#c95ad1',
  '#519c03',
  '#fb2b7b',
  '#6c3efd',
  '#f208c3',
  '#e46212',
  '#1da871',
  '#a609ea',
  '#de0784',
  '#f84713',
  '#169b4a',
  '#fb1597',
  '#c518e5',
  '#11ad32',
  // Patches, at 16 + the minor slot (first patch) or 16 + the minor slot + 8 (second).
  '#c865b5',
  '#c23913',
  '#c4236b',
  '#b1319d',
  '#cc5b1c',
  '#d5677c',
  '#ae54fc',
  '#d040b9',
  '#e83120',
  '#5da34d',
  '#a549dc',
  '#d91dfc',
  '#b905c5',
  '#298c01',
  '#cf4887',
  '#9322fb',
] as const

const minorSlotCount = 16

/** How far a second patch's slot sits from its first's, so it lands far from both neighbors' colors. */
const nextPatchStep = 8

function numericPart(part: string | undefined): number {
  return part !== undefined && /^\d+$/.test(part) ? Number(part) : 0
}

/**
 * A version's slot in `versionPalette`. Parts that aren't plain numbers (a pre-release suffix like
 * `0-beta`) read as zero, so `0.62.0-beta.1` shares `0.62.0`'s color.
 */
export function versionSlot(version: string): number {
  const [major, minor, patch] = version.replace(/^v/, '').split('.').map(numericPart)
  const minorSlot = (major * 1000 + minor) % minorSlotCount
  if (patch === 0) return minorSlot
  return minorSlotCount + ((minorSlot + nextPatchStep * (patch - 1)) % minorSlotCount)
}

/** The color for an app version like `0.61.0` (a leading `v` is ignored). */
export function versionColor(version: string): string {
  return versionPalette[versionSlot(version)]
}
