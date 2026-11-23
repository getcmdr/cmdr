/**
 * Which captured surface each key goes to: the first whose image SHOWS the key's text,
 * judged by OCR. The coupler (`couple-screenshots.ts`) builds its direct and not-shown
 * couplings from `placeKeys`; the text comes from `screenshot-ocr.ts` and the matching
 * from `screenshot-text-match.ts`. Pure: no filesystem.
 */

import { type OcrText } from './screenshot-ocr.ts'
import { type OcrDoc, isNeverVisible, ocrDoc, showsText } from './screenshot-text-match.ts'

/** One captured surface in the capture report: its screenshot and the keys that rendered on it. */
export interface CaptureSurface {
  screenshot: string
  keys: string[]
  /**
   * The UI zoom the driver had to photograph this surface at, present only when
   * it isn't the default 100%. Written by the capture driver for a surface too
   * tall to fit the display at full window height; surfaced in the coverage
   * report so a translator doesn't read that image as "this is the real size".
   */
  uiZoom?: number
}

/** The capture report: surface name → its screenshot + recorded keys. */
export type CaptureReport = Record<string, CaptureSurface>

/**
 * Whether a screenshot shows a key's text: `true` or `false` from its OCR, `null` when it
 * can't be judged (no OCR text for that image, a never-visible key, no literal text). A
 * `null` trusts the surface that looked the key up, which is the whole policy without OCR.
 */
export type ShowsVerdict = (key: string, screenshot: string) => boolean | null

/** Trusts every surface: the coupling without OCR. */
export const TRUST_EVERY_SURFACE: ShowsVerdict = () => null

/** Where the capture report places each key, judged by what each image shows. */
export interface ReportCouplings {
  /** Key → the first surface (report order) whose image shows its text, or can't be judged. */
  shown: Map<string, string>
  /** Key → the first surface that looked it up, for a key whose text no surface's image shows. */
  notShown: Map<string, string>
}

/**
 * Places each key of a capture report (surface → { screenshot, keys }) on the FIRST surface,
 * in the report's insertion order, whose image shows the key's text. A key every surface
 * resolved but none shows lands in `notShown`, on its first surface.
 *
 * Why the image decides: the report records what a surface LOOKED UP, which isn't what it
 * shows. A closed menu resolves its rows, a settings section every row below the fold, a tool
 * line its running words while it's photographed finished. An OCR pass over the 2026-10-31
 * images found a third of the keys missing from their own image under first-lookup-wins.
 */
export function placeKeys(report: CaptureReport, shows: ShowsVerdict = TRUST_EVERY_SURFACE): ReportCouplings {
  const shown = new Map<string, string>()
  const firstLookup = new Map<string, string>()
  for (const { screenshot, keys } of Object.values(report)) {
    for (const key of keys) {
      if (!firstLookup.has(key)) firstLookup.set(key, screenshot)
      if (!shown.has(key) && shows(key, screenshot) !== false) shown.set(key, screenshot)
    }
  }
  const notShown = new Map([...firstLookup].filter(([key]) => !shown.has(key)))
  return { shown, notShown }
}

/** The keys a report couples directly: `placeKeys`'s shown half. */
export function couplingsFromReport(
  report: CaptureReport,
  shows: ShowsVerdict = TRUST_EVERY_SURFACE,
): Map<string, string> {
  return placeKeys(report, shows).shown
}

/**
 * The note a key gets when no image shows its text: the surface that uses it is still the
 * right surroundings, but the picture must not pass for one of the string.
 *
 * DRAFT (David reviews translator-facing copy).
 */
export const NOT_SHOWN_NOTE =
  'This screenshot shows the screen your string belongs to, but not the string itself: it appears in another ' +
  'state of this screen (a menu or tooltip open, a step running, or further down a list). Take the wording from ' +
  'the description and the surroundings from this image.'

/**
 * The OCR verdict for the coupler, from the tracked OCR text and the English catalog.
 * Never-visible keys (accessible names, tooltips) and images with no OCR text answer `null`.
 */
export function ocrVerdict(
  ocrText: OcrText,
  english: Record<string, string>,
  descriptions: Record<string, string>,
): ShowsVerdict {
  const docs = new Map<string, OcrDoc>()
  return (key, screenshot) => {
    const entry = ocrText[screenshot] as OcrText[string] | undefined
    const value = english[key] as string | undefined
    if (entry === undefined || value === undefined || isNeverVisible(key, descriptions[key] ?? '')) return null
    let doc = docs.get(screenshot)
    if (doc === undefined) {
      doc = ocrDoc(entry.lines)
      docs.set(screenshot, doc)
    }
    return showsText(key, value, doc)
  }
}
