/**
 * Which keys commit and cancel a dialog, per the platform's convention (`enterActivates`,
 * `cancelKeys`), ❌ never the OS. `ModalDialog` applies them; a dialog only marks its default
 * button (`Button`'s `isDefault`). Mechanics and the per-dialog table: `DETAILS.md` § Platform
 * conventions.
 */

import { convention } from '#lib/platform/index.js'

/** The attribute `Button`'s `isDefault` writes (`''`), marking the dialog's default button. */
export const DEFAULT_BUTTON_ATTRIBUTE = 'data-dialog-default'

/**
 * Whether Return presses a focused button (Windows, GNOME, KDE: `focusedButton`), rather than
 * always the default one (macOS: `defaultButton`). A dialog that answers Return by giving its
 * safe button focus does that only where this is true; on the Mac, Return reaches no button
 * there (Apple's HIG: no default button when people should read first).
 */
export function enterPressesFocusedButton(): boolean {
  return convention('enterActivates') === 'focusedButton'
}

/** Whether ⌘. cancels, as Escape does (macOS). */
export function commandPeriodCancels(): boolean {
  return convention('cancelKeys') === 'escapeOrCommandPeriod'
}

/** Return with no modifier, outside an input method's composition. */
export function isPlainEnter(event: KeyboardEvent): boolean {
  return (
    event.key === 'Enter' && !event.isComposing && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey
  )
}

/** ⌘. (Command-period), with no other modifier. */
export function isCommandPeriod(event: KeyboardEvent): boolean {
  return event.key === '.' && event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey
}

/** A push button: what Space presses, and what Return presses on a `focusedButton` desktop. */
export function isButtonLike(target: EventTarget | null): target is HTMLElement {
  return (
    target instanceof HTMLButtonElement || (target instanceof HTMLElement && target.getAttribute('role') === 'button')
  )
}

/**
 * Controls whose own Return means something else than "commit the dialog", on every desktop: a
 * multi-line field (a new line), a list, grid, tree, or menu (its row), a link (follow it), and a
 * control that opens a popup or a region (a `Select` trigger, a disclosure).
 */
const OWNS_RETURN_SELECTOR = [
  'textarea',
  'select',
  'a[href]',
  '[contenteditable]:not([contenteditable="false"])',
  '[role="combobox"]',
  '[role="listbox"]',
  '[role="option"]',
  '[role="grid"]',
  '[role="gridcell"]',
  '[role="row"]',
  '[role="tree"]',
  '[role="treegrid"]',
  '[role="treeitem"]',
  '[role="menu"]',
  '[role="menuitem"]',
  '[aria-haspopup]:not([aria-haspopup="false"])',
  '[aria-expanded]',
].join(', ')

/** Whether Return pressed on `target` belongs to the control itself, not to the dialog's default button. */
export function ownsReturn(target: EventTarget | null): boolean {
  return target instanceof Element && target.matches(OWNS_RETURN_SELECTOR)
}

/** The class of `ModalDialog`'s footer, where the dialog's answers (its buttons) sit. */
const DIALOG_FOOTER_CLASS = 'modal-footer'

/**
 * Whether Return pressed on `target` presses `target` itself, as a browser does: any focused
 * button where Return presses the focused button (Windows, Linux); on the Mac, a widget's own
 * button (a `Select` trigger, a disclosure) and a button in the body (a recent path's row, a
 * suggestion chip, Copy), which stands for a list row or token. On the Mac Return on a footer
 * button, one of the dialog's answers, presses it only when it's the default one, and on any other
 * footer button does nothing (`ModalDialog`'s `handleReturn`).
 */
export function returnPressesTarget(target: EventTarget | null): boolean {
  if (!isButtonLike(target)) return false
  return enterPressesFocusedButton() || ownsReturn(target) || target.closest(`.${DIALOG_FOOTER_CLASS}`) === null
}

/** The dialog's default button in `panel`, when it can be pressed now (not disabled or blocked). */
export function defaultButtonIn(panel: HTMLElement | undefined): HTMLElement | null {
  const button = panel?.querySelector<HTMLElement>(`[${DEFAULT_BUTTON_ATTRIBUTE}]`) ?? null
  if (!button || button.matches(':disabled') || button.getAttribute('aria-disabled') === 'true') return null
  return button
}

/** Whether `target` is the dialog's default button (`Button`'s `isDefault`). */
export function isDefaultButton(target: HTMLElement): boolean {
  return target.hasAttribute(DEFAULT_BUTTON_ATTRIBUTE)
}
