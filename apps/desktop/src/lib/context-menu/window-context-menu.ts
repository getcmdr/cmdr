/**
 * The window's one in-page right-click menu host (`InPageContextMenus.svelte` mounts and registers
 * it where `convention('contextMenus')` is `inPage`), and the text-field menu it opens for
 * `#lib/context-menu-ownership.ts`. `DETAILS.md`.
 */

import { getPlatform } from '#lib/platform/index.js'
import { readClipboardText } from '#lib/tauri-commands/index.js'
import { isKeyboardContextMenu, type ContextMenuHost, type MenuPoint } from './context-menu-host.svelte'
import {
  readTextFieldFacts,
  restoreSelection,
  runTextFieldEdit,
  saveSelection,
  textFieldEditOf,
  textFieldMenuRows,
  type TextField,
} from './text-field-menu'

let host: ContextMenuHost | null = null

/** Registers the window's host; returns the unregister. One per window. */
export function setWindowContextMenuHost(next: ContextMenuHost): () => void {
  host = next
  return () => {
    if (host === next) host = null
  }
}

/** Where a text field's menu opens: the pointer, or for a key the point the engine named inside the field, else under it. */
function textMenuPoint(field: TextField, event: MouseEvent): MenuPoint {
  if (!isKeyboardContextMenu(event)) return { x: event.clientX, y: event.clientY }
  const rect = field.element.getBoundingClientRect()
  const inside =
    event.clientX >= rect.left &&
    event.clientX <= rect.right &&
    event.clientY >= rect.top &&
    event.clientY <= rect.bottom
  return inside ? { x: event.clientX, y: event.clientY } : { x: rect.left, y: rect.bottom }
}

/**
 * Opens `field`'s edit menu for the right-click (or Shift+F10, or the Menu key) `event`. Its
 * selection and caret stay as they were: a pick puts them back before it edits, and so does a
 * dismissal. The field keeps focus throughout. False when this window has no in-page menu.
 */
export function openTextFieldMenu(field: TextField, event: MouseEvent): boolean {
  if (!host) return false
  // Mid-composition the right-click is the input method's; a menu would cut across its keys.
  if (host.isComposing) return true
  const saved = saveSelection(field)
  let picked = false
  host.open({
    rows: textFieldMenuRows(readTextFieldFacts(field), getPlatform()),
    at: textMenuPoint(field, event),
    anchorElement: field.element,
    // The field keeps focus, so its blur handlers never run (an inline rename would cancel).
    focusHolder: field.element,
    onPick: async (menuId) => {
      picked = true
      const edit = textFieldEditOf(menuId)
      if (!edit) return
      restoreSelection(field, saved)
      await runTextFieldEdit(field, edit, { readClipboardText })
    },
    onSettled: () => {
      if (!picked && field.element.contains(document.activeElement)) restoreSelection(field, saved)
    },
  })
  return true
}
