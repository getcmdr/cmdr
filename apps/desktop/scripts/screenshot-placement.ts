/**
 * Which captured surface each key goes to: the first whose image SHOWS the key's text,
 * judged by OCR. The coupler (`couple-screenshots.ts`) builds its direct and not-shown
 * couplings from `placeKeys`; the text comes from `screenshot-ocr.ts` and the matching
 * from `screenshot-text-match.ts`. Pure: no filesystem.
 */

import { type OcrText } from './screenshot-ocr.ts'
import { isSearchKeywordsKey } from './i18n-catalog-lib.ts'
import { type OcrDoc, ocrDoc, showsText } from './screenshot-text-match.ts'

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
  'Your string isn’t visible in this screenshot. It shows up in another state of this screen, like an open menu ' +
  'or tooltip, a running step, or further down a list. Use the description for the meaning and this image for ' +
  'the context.'

/**
 * A `@key.visibility` value: how a string that never shows in a screenshot reaches the person.
 * Absent means the string is drawn on screen, so OCR can check it.
 *  - `tooltip`: shown on hover (or behind an ⓘ), which no capture opens.
 *  - `screenReader`: an accessible name, a screen-reader-only line, or an announcement.
 *  - `never`: not shown anywhere (an internal label for a hidden setting).
 */
export const VISIBILITIES = ['tooltip', 'screenReader', 'never'] as const
export type Visibility = (typeof VISIBILITIES)[number]

/** Whether a twin's raw `visibility` value is one of `VISIBILITIES`. */
export function isVisibility(value: unknown): value is Visibility {
  return typeof value === 'string' && (VISIBILITIES as readonly string[]).includes(value)
}

/**
 * The OCR verdict for the coupler, from the tracked OCR text and the English catalog. A key
 * whose `@key.visibility` says no picture shows it, a Settings search keyword list (never
 * drawn), and an image with no OCR text answer `null`: the surface that looked the key up is
 * the right context, and its words can't be checked.
 * @param visibility each key's declared `@key.visibility`, for the keys that carry one.
 */
export function ocrVerdict(
  ocrText: OcrText,
  english: Record<string, string>,
  visibility: ReadonlyMap<string, Visibility>,
): ShowsVerdict {
  const docs = new Map<string, OcrDoc>()
  return (key, screenshot) => {
    const entry = ocrText[screenshot] as OcrText[string] | undefined
    const value = english[key] as string | undefined
    if (entry === undefined || value === undefined || visibility.has(key) || isSearchKeywordsKey(key)) return null
    let doc = docs.get(screenshot)
    if (doc === undefined) {
      doc = ocrDoc(entry.lines)
      docs.set(screenshot, doc)
    }
    return showsText(key, value, doc)
  }
}
