/**
 * Focus watchdog: logs a WARN when the main window has OS focus but focus sits
 * in no keyboard home (neither file pane nor the Ask Cmdr rail) for
 * {@link WAIT_MS}+ ms, with no dialog open.
 *
 * Catches a class of bug where a modal or palette closes without restoring
 * focus to the previously focused pane. Arrow keys then silently no-op
 * because they go to `<body>`. The user just sees "keyboard nav broken"
 * with no obvious trigger; this logs the offending `activeElement` and the
 * likely cause (what last had focus, and whether it left the page, got
 * disabled, or lost it to a click on something unfocusable) so we can trace
 * it back to the component that should have kept or restored focus.
 *
 * Event-driven, no polling: `focusin`/`focusout` on document plus
 * `focus`/`blur` on window. Cost is a handful of listeners + at most one
 * timer (only while focus is misplaced). Warns at most once per "lost
 * focus episode"; resets when a pane regains focus.
 */

import { quickLookState } from '#lib/file-explorer/quick-look/quick-look-state.svelte.js'
import { getAppLogger } from '#lib/logging/logger.js'

const log = getAppLogger('focus-watchdog')

const WAIT_MS = 500

let timeoutId: number | null = null
let warned = false
let installed = false
/** The last element that held focus (never `<body>`), for naming what lost it. */
let lastFocused: Element | null = null
/** The last pointer press and when, for naming a click that put focus nowhere. */
let lastPointerDown: { target: Element; at: number } | null = null

/**
 * Selectors for every region that is a legitimate keyboard home: the dual-pane
 * explorer (the keyboard event sink for both file panes), the Ask Cmdr rail
 * (composer, sessions list, and its other focusable chrome), and an open in-app
 * menu (`ui/Menu.svelte`'s `[data-menu]` surface, which takes focus while it's up:
 * a Servers row menu), plus its `[data-menu-submenu]`, which takes focus while it
 * holds the cursor and is the surface's sibling, not its child. The in-page command menus
 * (`ui/command-menu/`, off macOS only) hold focus the same way: the in-window menu bar's
 * `[role="menubar"]`, whose title keeps focus while Alt or F10 has the bar, and each open
 * `[data-command-menu]` surface (a bar flyout, the main-menu button's menu, an in-page context
 * menu); the same pair `ui/focus-trap.ts` leaves alone. The rail and a menu only mount while
 * open, so a closed one matches nothing here without any extra guard.
 * The toast stack is always mounted but holds focus only when "Move focus to the
 * notifications" (or a click on a toast button) put it there.
 */
const KEYBOARD_HOME_SELECTOR =
  '.dual-pane-explorer, .ask-cmdr-rail, .status-corner, [data-menu], [data-menu-submenu], [role="menubar"], [data-command-menu], [data-toast-stack]'

/**
 * Returns true when `document.activeElement` lives inside a keyboard home. This
 * is the "focus is where it belongs" test: the whole watchdog fires only when
 * focus is in NONE of these homes (and no suppressor is active).
 */
function focusInsideKeyboardHome(): boolean {
  const ae = document.activeElement
  if (!ae || ae === document.body) return false
  return ae.closest(KEYBOARD_HOME_SELECTOR) !== null
}

/**
 * Returns true when any dialog-role element is mounted. Covers `ModalDialog`
 * (which sets `role="dialog"` or `role="alertdialog"`) AND the command
 * palette (which manages its own overlay with `role="dialog"`).
 */
function anyDialogOpen(): boolean {
  return document.querySelector('[role="dialog"], [role="alertdialog"]') !== null
}

function shouldSuppress(): boolean {
  // Quick Look's QLPreviewPanel is key while open, so our webview's
  // `document.activeElement` falls back to `<body>` and looks like
  // misplaced focus. AppKit will restore focus to the main window when the
  // panel closes, and the user is in control of when that happens (Shift+
  // Space, Esc, ✕). Treat the panel as the "dialog" for watchdog purposes.
  return !document.hasFocus() || focusInsideKeyboardHome() || anyDialogOpen() || quickLookState.isOpen
}

function check(): void {
  if (shouldSuppress()) {
    reset()
    return
  }
  // Focus is loose: start the countdown if we don't already have one in
  // flight and we haven't already warned for this episode.
  if (timeoutId === null && !warned) {
    timeoutId = window.setTimeout(() => {
      timeoutId = null
      // Re-check in case the situation resolved during the wait.
      if (shouldSuppress()) return
      warned = true
      const ae = document.activeElement
      log.warn('Focus left every keyboard home for {ms} ms with no dialog open: {cause}. activeElement={ae}', {
        ms: WAIT_MS,
        ae: ae ? describeElement(ae) : 'null',
        cause: describeCause(),
      })
    }, WAIT_MS)
  }
}

/** `tag#id.class1.class2.class3`: enough to find the component in the source. */
function describeElement(el: Element): string {
  const id = el.id ? `#${el.id}` : ''
  const classes =
    typeof el.className === 'string' && el.className
      ? `.${el.className.split(' ').filter(Boolean).slice(0, 3).join('.')}`
      : ''
  return `${el.tagName.toLowerCase()}${id}${classes}`
}

/**
 * Why focus is loose, from what had it last and the last pointer press. The usual causes, most
 * telling first: the focused element left the page (an `{#if}` swapped it out), it got
 * disabled, or a click landed on something WebKit won't focus (a button) with no focusable
 * ancestor, which drops focus to `<body>`.
 */
function describeCause(): string {
  const last = lastFocused
  if (last === null) return 'nothing had focus since the watchdog started'
  const lastText = describeElement(last)
  if (!last.isConnected) return `${lastText} left the page while it had focus`
  // `:disabled` covers every form control (button, input, textarea, select, fieldset).
  if (last.matches(':disabled')) return `${lastText} was disabled while it had focus`
  const pressed = lastPointerDown
  if (pressed !== null && pressed.target !== last && performance.now() - pressed.at < WAIT_MS * 2) {
    return `a click on ${describeElement(pressed.target)} took it from ${lastText}`
  }
  return `${lastText} lost it`
}

function onFocusIn(event: FocusEvent): void {
  const target = event.target
  if (target instanceof Element && target !== document.body) lastFocused = target
  check()
}

function onPointerDown(event: Event): void {
  if (event.target instanceof Element) lastPointerDown = { target: event.target, at: performance.now() }
}

function reset(): void {
  if (timeoutId !== null) {
    window.clearTimeout(timeoutId)
    timeoutId = null
  }
  warned = false
}

/**
 * Install the watchdog. Idempotent (calling twice is a no-op). Call once
 * from the main window's layout `onMount`.
 */
export function initFocusWatchdog(): void {
  if (installed) return
  installed = true
  // `focusin`/`focusout` bubble and fire on every focus change in the
  // document. Capture phase ensures we see them even if a handler down the
  // tree calls `stopPropagation`.
  document.addEventListener('focusin', onFocusIn, true)
  document.addEventListener('focusout', check, true)
  // Only remembered, for the log line's cause; it never starts a check on its own.
  document.addEventListener('pointerdown', onPointerDown, true)
  // Window-level focus/blur covers OS-level switches (other app, other window).
  window.addEventListener('focus', check)
  window.addEventListener('blur', reset)
  // Initial check after first paint: covers the case where focus is already
  // misplaced before the first focus change fires.
  window.setTimeout(check, 100)
}

/**
 * Test-only: tear down listeners and reset state so a fresh `initFocusWatchdog`
 * can run cleanly. jsdom's `document` is shared across tests and listeners
 * persist across module re-imports, so without this each test would accumulate
 * stale handlers that fire alongside the new ones.
 */
export function _resetForTests(): void {
  document.removeEventListener('focusin', onFocusIn, true)
  document.removeEventListener('focusout', check, true)
  document.removeEventListener('pointerdown', onPointerDown, true)
  lastFocused = null
  lastPointerDown = null
  window.removeEventListener('focus', check)
  window.removeEventListener('blur', reset)
  reset()
  installed = false
}
