// Appearance / system-environment wrappers: typed `on*` event listeners over the
// `tauri-specta` `events.*` helpers, plus the one-shot reads of the current OS
// appearance/accessibility state (accent color, display accessibility options, text-size
// multiplier, localized system strings).

import { type UnlistenFn } from '@tauri-apps/api/event'
import {
  commands,
  events,
  type AccentColorChanged,
  type DisplayAccessibility,
  type DisplayAccessibilityChanged,
  type GlassTintChanged,
  type LocalizedSystemStrings,
  type SystemTextSizeChanged,
  type OsLocales,
  type OsLocalesChanged,
  type SystemAccent,
} from '#lib/ipc/bindings.js'

/** Reads the current OS accent color as `#rrggbb`, plus the shades the OS fills controls with, where it names them. */
export function getAccentColor(): Promise<SystemAccent> {
  return commands.getAccentColor()
}

/**
 * Reads the macOS Accessibility > Display options the UI mirrors (Reduce transparency,
 * Increase contrast, Differentiate without color). All off outside macOS.
 */
export function getDisplayAccessibility(): Promise<DisplayAccessibility> {
  return commands.getDisplayAccessibility()
}

/**
 * Reads just the macOS "reduce transparency" state, for the window openers that pick a
 * native material before the window exists.
 */
export async function getShouldReduceTransparency(): Promise<boolean> {
  return (await commands.getDisplayAccessibility()).reduceTransparency
}

/**
 * Reads the macOS 27 Appearance > Liquid Glass slider: `0` clearest to `1` most tinted, or
 * `null` when macOS reports none.
 */
export function getGlassTintAmount(): Promise<number | null> {
  return commands.getGlassTintAmount()
}

/** Reads the current macOS Accessibility text-size multiplier (1.0 = default). */
export function getSystemTextSizeMultiplier(): Promise<number> {
  return commands.getSystemTextSizeMultiplier()
}

/** Reads OS-localized system strings (folder names, menu labels) for the app locale. */
export function getLocalizedSystemStrings(): Promise<LocalizedSystemStrings> {
  return commands.getLocalizedSystemStrings()
}

/**
 * Reads both OS locale answers: the UI language the preference list points at
 * (the first catalog we ship that the user can read), and the tag whose
 * conventions dates and numbers follow, composed from the OS's language and
 * region. Either half is `null` when the OS has no answer, which is both halves
 * off macOS, where the webview's own default stands.
 */
export function getOsLocales(): Promise<OsLocales> {
  return commands.getOsLocales()
}

/**
 * Subscribes to OS accent-color (or light/dark appearance) changes. The
 * payload's `accent` is the fresh answer, as `getAccentColor` returns it.
 */
export function onAccentColorChanged(handler: (payload: AccentColorChanged) => void): Promise<UnlistenFn> {
  return events.accentColorChanged.listen((event) => {
    handler(event.payload)
  })
}

/**
 * Subscribes to macOS Accessibility text-size changes. The payload's
 * `multiplier` is the new system text-size multiplier (1.0 = default).
 */
export function onSystemTextSizeChanged(handler: (payload: SystemTextSizeChanged) => void): Promise<UnlistenFn> {
  return events.systemTextSizeChanged.listen((event) => {
    handler(event.payload)
  })
}

/**
 * Subscribes to a live OS locale change: the user reordered their macOS
 * language preferences, or moved their region, and one of the two answers
 * actually moved. The payload's `locales` is the fresh pair, the same value
 * `getOsLocales()` would return now. Backend-deduplicated, so every event
 * carries a real change (`src-tauri/src/intl/live_locale.rs`).
 */
export function onOsLocalesChanged(handler: (payload: OsLocalesChanged) => void): Promise<UnlistenFn> {
  return events.osLocalesChanged.listen((event) => {
    handler(event.payload)
  })
}

/**
 * Subscribes to macOS Accessibility > Display option changes. The payload's `options`
 * is the fresh full set. Drives the `reduce-transparency`, `increase-contrast`, and
 * `differentiate-without-color` classes on `<html>` (see `#lib/display-accessibility`).
 */
export function onDisplayAccessibilityChanged(
  handler: (payload: DisplayAccessibilityChanged) => void,
): Promise<UnlistenFn> {
  return events.displayAccessibilityChanged.listen((event) => {
    handler(event.payload)
  })
}

/**
 * Subscribes to macOS 27 Liquid Glass slider moves, re-read each time the app becomes
 * active. The payload's `amount` is the new value, or `null` when macOS reports none.
 * Drives `--glass-tint` on `<html>` (see `#lib/glass-material`).
 */
export function onGlassTintChanged(handler: (payload: GlassTintChanged) => void): Promise<UnlistenFn> {
  return events.glassTintChanged.listen((event) => {
    handler(event.payload)
  })
}
