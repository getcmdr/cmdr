/**
 * How a child window with a glass page (Settings, the queue, Debug) opens: see-through over the
 * OS's own material, or opaque. See-through needs both a material the OS draws behind the webview
 * (the `windowMaterial` capability: macOS's `NSVisualEffectView`) and Reduce transparency off.
 * Anywhere else the window opens opaque in the system appearance, so the page's translucent fill
 * sits on a solid color.
 *
 * Gotcha/Why: the openers used to check Reduce transparency alone, so on Windows, where nothing
 * draws a material, Settings and the queue showed the windows behind them through the page
 * (real Windows 10 22H2, WebView2 157, 2026-11-19).
 */

import { hasCapability } from '#lib/platform/index.js'
import { getShouldReduceTransparency } from '#lib/tauri-commands/index.js'

export type WindowFill = [number, number, number, number]

export interface ChildWindowMaterial {
  /** Open `transparent` and apply the material with `setEffects` once the window exists. */
  glass: boolean
  /** The window's `backgroundColor`: clear behind the material, else the system appearance's fill. */
  backgroundColor: WindowFill
}

/** The opaque fill for the system appearance, matching `--color-bg-primary`. */
export function opaqueWindowFill(dark: boolean): WindowFill {
  return dark ? [30, 30, 30, 255] : [255, 255, 255, 255]
}

/** The material for a child window about to open. A failed read opens it opaque. */
export async function childWindowMaterial(
  readReduceTransparency: () => Promise<boolean> = getShouldReduceTransparency,
): Promise<ChildWindowMaterial> {
  let glass = false
  if (hasCapability('windowMaterial')) {
    try {
      glass = !(await readReduceTransparency())
    } catch {
      glass = false
    }
  }
  const dark = window.matchMedia('(prefers-color-scheme: dark)').matches
  return { glass, backgroundColor: glass ? [0, 0, 0, 0] : opaqueWindowFill(dark) }
}
