/**
 * Test helper for the dialog Return-safety tests (`DETAILS.md` § Platform conventions): presses
 * plain Return on every footer button of a dialog but the ones it's told to skip, the way someone
 * who Tabbed there would. The safety baseline is `origin/main` before the Windows port, where
 * Return on a focused button pressed that button and nothing else, so a test asserts that none of
 * these presses reaches the dialog's confirm callback.
 */

import type { ConventionsDesktop } from '#lib/platform/index.js'

export const ALL_CONVENTION_DESKTOPS: ConventionsDesktop[] = ['macos', 'windows', 'gnome', 'kde', 'otherLinux']

/** The footer buttons Return gets pressed on, so a test can say it reached any at all. */
export function footerButtonsOf(root: ParentNode): HTMLButtonElement[] {
  return [...root.querySelectorAll<HTMLButtonElement>('.modal-footer button')]
}

/**
 * Focuses each footer button `skip` doesn't name and presses plain Return there. happy-dom never
 * turns an unprevented Return into a click, so what reaches a callback is the dialog's own doing.
 * Returns how many buttons it pressed.
 */
export function pressReturnOnFooterButtons(root: ParentNode, skip: (button: HTMLButtonElement) => boolean): number {
  let pressed = 0
  for (const button of footerButtonsOf(root)) {
    if (skip(button) || button.disabled) continue
    button.focus()
    button.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true, cancelable: true }))
    pressed++
  }
  return pressed
}
