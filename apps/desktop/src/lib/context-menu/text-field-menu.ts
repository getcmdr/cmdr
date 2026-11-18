/**
 * A text field's right-click menu where the page draws its menus (`contextMenus: 'inPage'`,
 * Windows): Undo, Redo, Cut, Copy, Paste, and Select all, drawn by the same `ContextMenu` as every
 * other menu, so the app's menus all look alike (David, 2026-11-18). Where menus are native, the
 * engine's own edit menu stays (`#lib/context-menu-ownership.ts`).
 *
 * Decision/Why: these rows live here, ❌ not in `cmdr-menu`. They aren't Cmdr commands: they stand
 * in for the engine's own edit menu, and each acts on the field in the page, which only the page
 * can do. A backend round trip would also cost Chromium's user activation, without which
 * `execCommand('copy')` and `'cut'` do nothing. `DETAILS.md` § "The text-field menu".
 */

import type { Row } from '#lib/ipc/bindings.js'
import type { MessageKey } from '#lib/intl/keys.gen.js'
import type { Platform } from '#lib/platform/index.js'

/** What a text-field menu row does to the field. */
export type TextFieldEdit = 'undo' | 'redo' | 'cut' | 'copy' | 'paste' | 'selectAll'

/** A field a text menu acts on: a text `<input>` or `<textarea>`, or a `contenteditable` host. */
export type TextField =
  | { kind: 'control'; element: HTMLInputElement | HTMLTextAreaElement }
  | { kind: 'editable'; element: HTMLElement }

/** What a right-click landed on, as far as text editing goes. */
export type TextTarget =
  /** A text field that takes a menu. */
  | { kind: 'field'; field: TextField }
  /** A form control with no text menu: a disabled field, a checkbox, a slider, a `<select>`. */
  | { kind: 'control' }
  /** Not a form control at all. */
  | { kind: 'none' }

/** The `<input>` types that hold editable text. `number` and `email` keep no selection range, which `readTextFieldFacts` allows for. */
const TEXT_INPUT_TYPES = new Set(['text', 'search', 'url', 'tel', 'email', 'password', 'number'])

/** The `contenteditable` host `node` sits in: the outermost editable ancestor, so an edit spans the whole field. */
function editingHost(node: HTMLElement): HTMLElement {
  let host = node
  while (host.parentElement?.isContentEditable) host = host.parentElement
  return host
}

/** What `target` is, for a right-click: a text field with a menu, another form control, or neither. */
export function textTargetOf(target: EventTarget | null): TextTarget {
  if (target instanceof HTMLTextAreaElement) {
    return target.matches(':disabled')
      ? { kind: 'control' }
      : { kind: 'field', field: { kind: 'control', element: target } }
  }
  if (target instanceof HTMLInputElement) {
    if (!TEXT_INPUT_TYPES.has(target.type) || target.matches(':disabled')) return { kind: 'control' }
    return { kind: 'field', field: { kind: 'control', element: target } }
  }
  if (target instanceof HTMLSelectElement) return { kind: 'control' }
  if (target instanceof HTMLElement && target.isContentEditable) {
    return { kind: 'field', field: { kind: 'editable', element: editingHost(target) } }
  }
  return { kind: 'none' }
}

/** What the menu's rows need to know about the field, read as it opens. */
export interface TextFieldFacts {
  /** Not read-only: Undo, Redo, Cut, and Paste can change it. */
  editable: boolean
  /** Some of its text is selected (or the field can't say, as `number` and `email` inputs can't). */
  hasSelection: boolean
  /** It holds any text, so Select all has something to select. */
  hasText: boolean
  /** A password field: its text never goes to the clipboard, as the engine's own menu greys Cut and Copy. */
  concealed: boolean
  canUndo: boolean
  canRedo: boolean
}

/** Whether the engine would run `command` in the focused field now; true when it can't say. */
function commandEnabled(command: 'undo' | 'redo'): boolean {
  try {
    // eslint-disable-next-line @typescript-eslint/no-deprecated -- the one way a page asks whether the focused field has an undo step; no modern API reads the field's undo stack
    return document.queryCommandEnabled(command)
  } catch {
    return true
  }
}

/** Whether `element` has text selected; true when its type keeps no selection range (`number`, `email`). */
function controlHasSelection(element: HTMLInputElement | HTMLTextAreaElement): boolean {
  try {
    const { selectionStart, selectionEnd } = element
    return selectionStart === null || selectionEnd === null || selectionStart !== selectionEnd
  } catch {
    // A type with no selection range reads as "maybe": an enabled Copy that copies nothing beats a dead one.
    return true
  }
}

function controlFacts(element: HTMLInputElement | HTMLTextAreaElement): TextFieldFacts {
  const editable = !element.readOnly
  const undoable = editable && document.activeElement === element
  return {
    editable,
    hasSelection: controlHasSelection(element),
    hasText: element.value.length > 0,
    concealed: element instanceof HTMLInputElement && element.type === 'password',
    canUndo: undoable && commandEnabled('undo'),
    canRedo: undoable && commandEnabled('redo'),
  }
}

function editableFacts(element: HTMLElement): TextFieldFacts {
  const selection = window.getSelection()
  const anchor = selection?.anchorNode ?? null
  const focused = element.contains(document.activeElement)
  return {
    editable: true,
    hasSelection: selection !== null && !selection.isCollapsed && anchor !== null && element.contains(anchor),
    hasText: element.textContent.length > 0,
    concealed: false,
    canUndo: focused && commandEnabled('undo'),
    canRedo: focused && commandEnabled('redo'),
  }
}

/** Reads `field`'s facts. Ask while it still has focus: undo state is the focused field's. */
export function readTextFieldFacts(field: TextField): TextFieldFacts {
  return field.kind === 'control' ? controlFacts(field.element) : editableFacts(field.element)
}

/** Each edit's row id. Page-local: these rows never reach the backend. */
const ROW_IDS: Record<TextFieldEdit, string> = {
  undo: 'text_field_undo',
  redo: 'text_field_redo',
  cut: 'text_field_cut',
  copy: 'text_field_copy',
  paste: 'text_field_paste',
  selectAll: 'text_field_select_all',
}

/** The edit a row id names, or `null` for an id that isn't one of this menu's. */
export function textFieldEditOf(menuId: string): TextFieldEdit | null {
  for (const [edit, id] of Object.entries(ROW_IDS) as [TextFieldEdit, string][]) if (id === menuId) return edit
  return null
}

/** Each edit's label: the Edit menu's own words. */
const LABELS: Record<TextFieldEdit, MessageKey> = {
  undo: 'menu.edit.undo',
  redo: 'menu.edit.redo',
  cut: 'menu.edit.cut',
  copy: 'menu.edit.copy',
  paste: 'menu.edit.paste',
  selectAll: 'menu.select.all',
}

/** The engine's own keys for each edit: shown as `[shortcut, aria-keyshortcuts]`. */
function editKeys(edit: TextFieldEdit, platform: Platform): [string, string] {
  if (platform === 'macos') {
    const mac: Record<TextFieldEdit, [string, string]> = {
      undo: ['⌘Z', 'Meta+Z'],
      redo: ['⇧⌘Z', 'Meta+Shift+Z'],
      cut: ['⌘X', 'Meta+X'],
      copy: ['⌘C', 'Meta+C'],
      paste: ['⌘V', 'Meta+V'],
      selectAll: ['⌘A', 'Meta+A'],
    }
    return mac[edit]
  }
  // Windows' edit controls redo on Ctrl+Y; GTK and Qt fields on Ctrl+Shift+Z.
  const redo: [string, string] = platform === 'windows' ? ['Ctrl+Y', 'Control+Y'] : ['Ctrl+Shift+Z', 'Control+Shift+Z']
  const others: Record<TextFieldEdit, [string, string]> = {
    undo: ['Ctrl+Z', 'Control+Z'],
    redo,
    cut: ['Ctrl+X', 'Control+X'],
    copy: ['Ctrl+C', 'Control+C'],
    paste: ['Ctrl+V', 'Control+V'],
    selectAll: ['Ctrl+A', 'Control+A'],
  }
  return others[edit]
}

/** Whether `edit` does something to a field with `facts`. */
export function editEnabled(edit: TextFieldEdit, facts: TextFieldFacts): boolean {
  switch (edit) {
    case 'undo':
      return facts.editable && facts.canUndo
    case 'redo':
      return facts.editable && facts.canRedo
    case 'cut':
      return facts.editable && facts.hasSelection && !facts.concealed
    case 'copy':
      return facts.hasSelection && !facts.concealed
    case 'paste':
      return facts.editable
    case 'selectAll':
      return facts.hasText
  }
}

/** The menu's rows: Undo and Redo, Cut, Copy, and Paste, then Select all, each greyed where it would do nothing. */
export function textFieldMenuRows(facts: TextFieldFacts, platform: Platform): Row[] {
  const item = (edit: TextFieldEdit): Row => {
    const [shortcut, ariaKeyshortcuts] = editKeys(edit, platform)
    return {
      kind: 'item',
      id: ROW_IDS[edit],
      label: { key: LABELS[edit], args: {} },
      shortcut,
      ariaKeyshortcuts,
      enabled: editEnabled(edit, facts),
    }
  }
  return [
    item('undo'),
    item('redo'),
    { kind: 'separator' },
    item('cut'),
    item('copy'),
    item('paste'),
    { kind: 'separator' },
    item('selectAll'),
  ]
}

/** The field's selection as the menu opened, put back before an edit runs (the menu took focus meanwhile). */
export type SavedSelection =
  | { kind: 'control'; start: number; end: number; direction: 'forward' | 'backward' | 'none' }
  | { kind: 'editable'; range: Range }
  | null

export function saveSelection(field: TextField): SavedSelection {
  if (field.kind === 'control') {
    try {
      const { selectionStart, selectionEnd, selectionDirection } = field.element
      if (selectionStart === null || selectionEnd === null) return null
      return { kind: 'control', start: selectionStart, end: selectionEnd, direction: selectionDirection ?? 'none' }
    } catch {
      return null
    }
  }
  const selection = window.getSelection()
  if (!selection || selection.rangeCount === 0) return null
  const range = selection.getRangeAt(0)
  return field.element.contains(range.commonAncestorContainer) ? { kind: 'editable', range: range.cloneRange() } : null
}

export function restoreSelection(field: TextField, saved: SavedSelection): void {
  if (!saved) return
  if (saved.kind === 'control' && field.kind === 'control') {
    try {
      field.element.setSelectionRange(saved.start, saved.end, saved.direction)
    } catch {
      // The field's type stopped taking a range meanwhile: leave the caret where it is.
    }
    return
  }
  if (saved.kind === 'editable') {
    const selection = window.getSelection()
    selection?.removeAllRanges()
    selection?.addRange(saved.range)
  }
}

export interface TextFieldEditDeps {
  /** The clipboard's text, read by the backend (no engine permission prompt). */
  readClipboardText: () => Promise<string | null>
}

/** Runs `edit` in `field`, which has focus back. Synchronous but for Paste, so Cut and Copy keep the pick's user activation. */
export async function runTextFieldEdit(field: TextField, edit: TextFieldEdit, deps: TextFieldEditDeps): Promise<void> {
  switch (edit) {
    case 'undo':
    case 'redo':
    case 'cut':
    case 'copy':
      // eslint-disable-next-line @typescript-eslint/no-deprecated -- the one way a page hands an edit to the focused field, as the engine's own menu does
      document.execCommand(edit)
      return
    case 'paste': {
      const text = await deps.readClipboardText()
      if (!text || !field.element.isConnected) return
      // eslint-disable-next-line @typescript-eslint/no-deprecated -- insertText is the only way to insert at the caret and keep the field's undo step
      document.execCommand('insertText', false, text)
      return
    }
    case 'selectAll':
      if (field.kind === 'control') field.element.select()
      else window.getSelection()?.selectAllChildren(field.element)
      return
  }
}
