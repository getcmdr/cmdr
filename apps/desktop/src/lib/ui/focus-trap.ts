/**
 * `use:trapFocus` — keeps keyboard focus inside a modal container while it's mounted.
 *
 * Every dialog-role overlay in the app uses this action. It does three jobs:
 *
 * 1. **Tab wrapping**: Tab on the last tabbable wraps to the first; Shift+Tab on the
 *    first wraps to the last. The tabbable list is queried fresh on every keypress, so
 *    dialogs whose controls mount and unmount (the onboarding wizard, filter popovers)
 *    stay trapped without re-registration.
 * 2. **Leak guard**: if focus lands outside the container anyway (a programmatic
 *    `.focus()` from pane code finishing an async refresh, for example), it gets pulled
 *    back to the last-focused element inside the container.
 * 3. **Escape fallback**: if an Escape keydown fires while focus is outside the
 *    container — the broken state the leak guard exists for — `onEscape` runs so the
 *    user can always close the dialog from the keyboard. When focus is inside (the
 *    healthy state), this action stays out of the way and the dialog's own Escape
 *    handler works as usual.
 *
 * Traps stack: when several are mounted (a filter-chip popover inside the search
 * dialog), only the most recently mounted one enforces. Closing the top trap hands
 * enforcement back to the one below, which is what gives nested popovers their
 * "Esc closes only the popover" semantics for the leaked-focus path.
 *
 * All listeners run in the capture phase, so an inner component calling
 * `stopPropagation()` on keydown (which every dialog does to shield the file explorer)
 * can't starve the trap.
 */

import type { ActionReturn } from 'svelte/action'

export interface TrapFocusOptions {
  /**
   * Called when Escape is pressed while focus has leaked outside the container.
   * Wire it to the dialog's close callback. Omit for dialogs that intentionally
   * swallow Escape (the onboarding wizard).
   */
  onEscape?: () => void
  /**
   * `false` parks the trap: it stays mounted but doesn't enforce, and the next trap down the stack
   * does. For a passive popover that follows a field outside it (`Popover`'s `passive`). Default true.
   */
  enabled?: boolean
}

/**
 * No `offsetParent` visibility filtering: jsdom/happy-dom return `null` for every
 * element, which would empty the list in tests. Conditionally rendered controls
 * aren't in the DOM at all, and `[disabled]`, an explicit `tabindex="-1"`, and
 * `[hidden]` are excluded, so the list matches what a real user can tab to.
 *
 * ❌ Don't drop the `:not([tabindex="-1"])` from the natively-focusable clauses:
 * a control that opted OUT of the tab order is still a `<button>` / `<select>`.
 * Ark's `Select` renders exactly that (a visually-hidden `<select tabindex="-1">`
 * mirroring the value for form submission), and its closed menu is a `[hidden]`
 * `tabindex="0"` listbox. Without both guards a wizard's Tab wrap lands on one of
 * them and focus goes nowhere the user can see.
 */
const FOCUSABLE_SELECTOR = [
  '[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]',
]
  .map((clause) => `${clause}:not([tabindex="-1"])`)
  .join(', ')

interface TrapEntry {
  node: HTMLElement
  options: TrapFocusOptions
  /** Where to send focus when pulling it back after a leak. */
  lastFocusedInside: HTMLElement | null
}

const stack: TrapEntry[] = []

/** The in-page command menus (`command-menu/`): the bar, its flyouts, and the context menus. */
const COMMAND_MENU_SELECTOR = '[role="menubar"], [data-command-menu]'
let listenersInstalled = false

/**
 * Marks a row whose children are laid out in reverse (`flex-direction: row-reverse`), so Tab
 * walks them in the order the eye reads them rather than the DOM's. `ModalDialog` sets it on its
 * footer row where the platform puts the affirmative button first (`dialogButtonOrder`).
 */
export const REVERSED_FOCUS_ORDER_ATTRIBUTE = 'data-focus-order'
const REVERSED_ROW_SELECTOR = `[${REVERSED_FOCUS_ORDER_ATTRIBUTE}="reverse"]`

function getTabbables(node: HTMLElement): HTMLElement[] {
  // `[hidden]` is checked on ancestors too: a closed Ark dropdown hides the wrapper,
  // not always the tabbable inside it. A `.focus()` on any of them is a silent no-op in
  // a real browser, which would strand the wrap on an element the user can't see.
  const inDomOrder = Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (el) => el.closest('[hidden]') === null,
  )
  return inReadingOrder(inDomOrder, node)
}

/**
 * `tabbables` with each reversed row's controls in the order it shows them: the row's direct
 * children go last to first, and the controls inside one child keep their own order, the same
 * granularity `row-reverse` flips.
 */
function inReadingOrder(tabbables: HTMLElement[], node: HTMLElement): HTMLElement[] {
  const rows = Array.from(node.querySelectorAll<HTMLElement>(REVERSED_ROW_SELECTOR))
  if (rows.length === 0) return tabbables
  let ordered = tabbables
  for (const row of rows) {
    const inRow = ordered.filter((el) => row.contains(el))
    if (inRow.length < 2) continue
    const childOf = (el: HTMLElement): Element | null => {
      let child: Element | null = el
      while (child && child.parentElement !== row) child = child.parentElement
      return child
    }
    const children = Array.from(row.children)
    const flipped = [...inRow].sort((a, b) => {
      const byChild = children.indexOf(childOf(b) as Element) - children.indexOf(childOf(a) as Element)
      return byChild !== 0 ? byChild : inRow.indexOf(a) - inRow.indexOf(b)
    })
    // The row's controls sit together in DOM order, so they take back the same slots.
    const start = ordered.indexOf(inRow[0])
    ordered = [...ordered.slice(0, start), ...flipped, ...ordered.slice(start + inRow.length)]
  }
  return ordered
}

/**
 * A field a person can type into: text-like `<input>`s and `<textarea>`s that are
 * enabled, writable, and in the tab order. ❌ Not buttons, checkboxes, or radios:
 * Enter on a focused button activates THAT button instead of the dialog's default
 * action (`ModalDialog`'s Enter handling), which a dialog that opened on one would
 * turn into a trap.
 */
const TYPEABLE_SELECTOR = [
  'input:not([type]):not([disabled]):not([readonly])',
  ...['text', 'search', 'password', 'email', 'url', 'tel', 'number'].map(
    (type) => `input[type="${type}"]:not([disabled]):not([readonly])`,
  ),
  'textarea:not([disabled]):not([readonly])',
]
  .map((clause) => `${clause}:not([tabindex="-1"])`)
  .join(', ')

/**
 * Focuses the first field in `container` a person can type into, answering
 * whether there was one.
 *
 * ❗ How a dialog opens ready to type, whatever opened it (a context menu, a pane
 * shortcut, a button): `ModalDialog` calls it when nothing inside claimed focus,
 * and a dialog that knows which field it wants calls it on the part of itself it
 * means. ❌ Don't `.focus()` a field that may be locked instead: focusing a
 * disabled input is a silent no-op, and the dialog then opens with no field taking
 * keys ("Edit server…" did, with Address locked).
 */
export function focusFirstField(container: HTMLElement): boolean {
  const field = Array.from(container.querySelectorAll<HTMLElement>(TYPEABLE_SELECTOR)).find(
    (el) => el.closest('[hidden]') === null,
  )
  field?.focus()
  return field !== undefined
}

/** Last-focused element inside the trap, falling back to the first tabbable, then the container. */
function restoreFocus(entry: TrapEntry): void {
  const { node, lastFocusedInside } = entry
  if (lastFocusedInside?.isConnected && node.contains(lastFocusedInside)) {
    lastFocusedInside.focus()
    return
  }
  const tabbables = getTabbables(node)
  if (tabbables.length > 0) {
    tabbables[0].focus()
    return
  }
  // Overlay containers carry tabindex="-1" so this works even for an empty dialog.
  node.focus()
}

/** The trap that enforces: the most recently mounted one that isn't parked. */
function topTrap(): TrapEntry | undefined {
  return stack.findLast((entry) => entry.options.enabled !== false)
}

function handleKeyDown(e: KeyboardEvent): void {
  const top = topTrap()
  if (!top) return
  const target = e.target instanceof HTMLElement ? e.target : null
  const inside = target !== null && top.node.contains(target)

  if (e.key === 'Tab') {
    if (!inside) {
      // Focus already leaked; don't let Tab walk it further away.
      e.preventDefault()
      restoreFocus(top)
      return
    }
    // `inside` narrows `target` to a non-null HTMLElement (aliased-condition narrowing).
    wrapTab(e, top.node, target)
    return
  }

  if (e.key === 'Escape' && !inside && top.options.onEscape) {
    // The dialog's own Escape handler can't fire (focus is outside the container, so
    // the event never propagates through it). Close from here so the keyboard always works.
    e.preventDefault()
    e.stopPropagation()
    top.options.onEscape()
  }
}

/**
 * What the browser's own Tab would stop on, for the Tabs this trap moves itself: a rendered
 * control, and one radio per group (the checked one, else the first), since arrows move inside
 * a group. `checkVisibility` is missing in jsdom, where everything counts as rendered.
 */
function isTabStop(el: HTMLElement): boolean {
  if (typeof el.checkVisibility === 'function' && !el.checkVisibility()) return false
  if (!(el instanceof HTMLInputElement) || el.type !== 'radio' || el.name === '') return true
  const group = Array.from(
    (el.form ?? el.ownerDocument).querySelectorAll<HTMLInputElement>('input[type="radio"]'),
  ).filter((radio) => radio.name === el.name)
  const checked = group.find((radio) => radio.checked)
  return el === (checked ?? group[0])
}

/** Wraps Tab at the boundaries of the container's tabbable cycle. */
function wrapTab(e: KeyboardEvent, node: HTMLElement, target: HTMLElement): void {
  const tabbables = getTabbables(node)
  if (tabbables.length === 0) {
    e.preventDefault()
    return
  }
  const index = tabbables.indexOf(target)
  const goingForward = !e.shiftKey
  if (node.querySelector(REVERSED_ROW_SELECTOR) !== null) {
    // A reversed row makes the reading order differ from the DOM's, so the browser's own
    // mid-list step would walk that row backwards: every Tab moves here.
    e.preventDefault()
    const stops = tabbables.filter((el) => el === target || isTabStop(el))
    if (stops.length === 0) return
    const at = stops.indexOf(target)
    const next = goingForward
      ? at === -1 || at === stops.length - 1
        ? 0
        : at + 1
      : at <= 0
        ? stops.length - 1
        : at - 1
    stops[next].focus()
    return
  }
  if (goingForward && (index === -1 || index === tabbables.length - 1)) {
    // From the last tabbable, the container itself, or a click-focused
    // tabindex="-1" element: wrap to the first.
    e.preventDefault()
    tabbables[0].focus()
  } else if (!goingForward && index <= 0) {
    e.preventDefault()
    tabbables[tabbables.length - 1].focus()
  }
  // Mid-list Tab stays with the browser: the next tabbable in DOM order is inside the container.
}

function handleFocusIn(e: FocusEvent): void {
  const top = topTrap()
  if (!top) return
  const target = e.target instanceof HTMLElement ? e.target : null
  if (!target) return
  if (top.node.contains(target)) {
    top.lastFocusedInside = target
    return
  }
  // A command menu over the dialog (the Mac bar and its menus, which stay usable over a sheet, or
  // a context menu) holds focus for its life and gives it back itself on a pick or Escape.
  if (target.closest(COMMAND_MENU_SELECTOR) !== null) return
  // Focus escaped. Pull it back after a microtask, not synchronously: when a dialog
  // closes, its onDestroy restores focus to the pane while this trap may still be
  // registered for the remainder of the teardown. The microtask runs after the
  // action's destroy has unregistered the trap, so a closing dialog never yanks
  // focus back into its own dying DOM.
  queueMicrotask(() => {
    if (topTrap() !== top || !top.node.isConnected) return
    const active = document.activeElement
    if (active instanceof HTMLElement && top.node.contains(active)) return
    restoreFocus(top)
  })
}

function installListeners(): void {
  if (listenersInstalled) return
  listenersInstalled = true
  document.addEventListener('keydown', handleKeyDown, true)
  document.addEventListener('focusin', handleFocusIn, true)
}

function removeListenersIfIdle(): void {
  if (stack.length > 0 || !listenersInstalled) return
  listenersInstalled = false
  document.removeEventListener('keydown', handleKeyDown, true)
  document.removeEventListener('focusin', handleFocusIn, true)
}

/** Svelte action. Apply to the dialog's outermost element (the overlay). */
export function trapFocus(node: HTMLElement, options: TrapFocusOptions = {}): ActionReturn<TrapFocusOptions> {
  const entry: TrapEntry = { node, options, lastFocusedInside: null }
  stack.push(entry)
  installListeners()

  return {
    update(newOptions: TrapFocusOptions) {
      entry.options = newOptions
    },
    destroy() {
      const index = stack.indexOf(entry)
      if (index !== -1) stack.splice(index, 1)
      removeListenersIfIdle()
    },
  }
}

/**
 * Test-only: drop all traps and detach the document listeners. The stack is
 * module-level state shared across tests in a file; a test that fails before
 * unmounting would otherwise leak its trap into the next test.
 */
export function _resetForTests(): void {
  stack.length = 0
  removeListenersIfIdle()
}
