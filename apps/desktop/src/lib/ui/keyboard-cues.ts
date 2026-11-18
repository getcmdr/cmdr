/**
 * When access-key underlines show, for menus and dialogs alike (task 63 of
 * `docs/specs/windows-port-plan.md`, D5's third point). The rule is the platform's, read from
 * `convention('accessKeys')`, ❌ never the OS:
 *
 * - `afterKeyboardUse` (Windows, Win32's keyboard-cue rule): a surface opened from the keyboard
 *   shows them from the start; one opened by a click hides them until a key is used in it (Alt,
 *   F10, Tab, an arrow, an Alt chord), and from then on they stay for the surface's life.
 * - `whileAltHeld` (every Linux desktop, as GTK and Qt draw them): shown while Alt is held alone,
 *   gone on its release, any other key, a click, or the window losing focus.
 * - `none` (macOS): never.
 *
 * Windows' "Underline access keys" setting (the `underline-access-keys` class on `<html>`,
 * `#lib/display-accessibility`) shows them always, followed live. Mechanics: `DETAILS.md` §
 * Keyboard cues.
 */

import { convention } from '#lib/platform/conventions.js'
import { isAltGraphHeld } from '#lib/shortcuts/key-capture.js'

export type CueRule = 'afterKeyboardUse' | 'whileAltHeld' | 'none'

/** The cue rule of this platform's access keys. */
export function cueRule(): CueRule {
  switch (convention('accessKeys')) {
    case 'underlinesAfterKeyboardUse':
      return 'afterKeyboardUse'
    case 'underlinesWhileAltHeld':
      return 'whileAltHeld'
    case 'none':
      return 'none'
  }
}

/** The class Windows' "Underline access keys" setting puts on `<html>`. */
const ALWAYS_CLASS = 'underline-access-keys'

let lastKeyboard = false
let tracking = false

/**
 * Starts following the window's last input, once per document. Capture phase, so a handler
 * that stops a key can't hide it: whether a surface opened from the keyboard is decided by the
 * key that opened it.
 */
function trackInput(): void {
  if (tracking || typeof window === 'undefined') return
  tracking = true
  window.addEventListener(
    'keydown',
    () => {
      lastKeyboard = true
    },
    true,
  )
  window.addEventListener(
    'pointerdown',
    () => {
      lastKeyboard = false
    },
    true,
  )
}
trackInput()

/** Whether the window's last input was a key, which is how a surface opening now tells it was opened from the keyboard. */
export function lastInputWasKeyboard(): boolean {
  return lastKeyboard
}

function alwaysOn(): boolean {
  return typeof document !== 'undefined' && document.documentElement.classList.contains(ALWAYS_CLASS)
}

/** Alt pressed alone: never AltGr (Ctrl+Alt on Windows), which types characters. */
function isLoneAlt(event: KeyboardEvent): boolean {
  return event.key === 'Alt' && !event.ctrlKey && !event.metaKey && !event.shiftKey && !isAltGraphHeld(event)
}

/** The keys Win32 counts as using the keyboard in a surface (`IsDialogMessage`'s set). */
function isCueKey(event: KeyboardEvent): boolean {
  if (event.key === 'AltGraph' || isAltGraphHeld(event) || (event.altKey && event.ctrlKey)) return false
  if (event.key === 'Alt' || event.key === 'Tab' || event.key === 'F10' || event.key.startsWith('Arrow')) return true
  return event.altKey && !event.metaKey
}

export interface KeyboardCues {
  /** Whether the surface shows its underlines now. */
  readonly shown: boolean
  /** A keydown in the surface. */
  keydown(event: KeyboardEvent): void
  /** A keyup in the surface (Alt's release ends `whileAltHeld`). */
  keyup(event: KeyboardEvent): void
  /** A pointer press, anywhere. */
  pointerdown(): void
  /** The window lost focus: WebView2 sends no Alt keyup then, so held Alt resets. */
  blur(): void
  /** The keyboard was used, by the surface's own measure (a menu counts every key it handles). */
  reveal(): void
  /** Calls `listener` whenever `shown` flips; returns the unsubscribe. */
  onChange(listener: (shown: boolean) => void): () => void
  /** Stops following the setting. */
  destroy(): void
}

export interface KeyboardCuesOptions {
  /** Defaults to this platform's ({@link cueRule}). */
  rule?: CueRule
  /** Whether the surface opened from the keyboard. Defaults to {@link lastInputWasKeyboard}. */
  openedByKeyboard?: boolean
}

/** One surface's cue state (a menu bar, a menu, a dialog). */
export function createKeyboardCues(options: KeyboardCuesOptions = {}): KeyboardCues {
  const rule = options.rule ?? cueRule()
  let used = rule === 'afterKeyboardUse' && (options.openedByKeyboard ?? lastInputWasKeyboard())
  let altHeld = false
  let always = alwaysOn()
  const listeners = new Set<(shown: boolean) => void>()

  const compute = (): boolean => rule !== 'none' && (always || used || altHeld)
  let shown = compute()

  function update(): void {
    const next = compute()
    if (next === shown) return
    shown = next
    for (const listener of listeners) listener(shown)
  }

  const observer =
    typeof MutationObserver === 'undefined' || typeof document === 'undefined'
      ? null
      : new MutationObserver(() => {
          always = alwaysOn()
          update()
        })
  observer?.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })

  return {
    get shown() {
      return shown
    },
    keydown(event) {
      if (rule === 'afterKeyboardUse') {
        if (isCueKey(event)) used = true
      } else if (rule === 'whileAltHeld') {
        // Any other key while Alt is down (an Alt chord) ends the cue, as GTK's does.
        altHeld = isLoneAlt(event)
      }
      update()
    },
    keyup(event) {
      if (event.key === 'Alt' || event.key === 'AltGraph') altHeld = false
      update()
    },
    pointerdown() {
      altHeld = false
      update()
    },
    blur() {
      altHeld = false
      update()
    },
    reveal() {
      if (rule === 'afterKeyboardUse') used = true
      update()
    },
    onChange(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    destroy() {
      observer?.disconnect()
      listeners.clear()
    },
  }
}
