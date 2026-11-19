/**
 * An in-page right-click menu's behavior (`ContextMenu.svelte` draws it): one `menu-stack` opened
 * at a point, with a context menu's keys. ↑/↓ walk the live rows, Home/End jump, → enters a
 * submenu and ← or Esc leaves it, Esc then closes the menu, Enter / Space pick, Tab closes, and on
 * the `desktop` flavor a letter picks the row it underlines. A menu opened from the keyboard
 * (Shift+F10, the Menu key) starts on its first live row. Every key goes to the open menu, never
 * the app behind it.
 *
 * It closes, without moving focus, on a click elsewhere, the window losing focus or resizing, a
 * scroll of what it was opened on, and a dialog opening. A pick or Esc puts focus back where it
 * was. Mechanics: `../DETAILS.md` § Command menus.
 */

import type { Row } from '#lib/ipc/bindings.js'
import { captureFocusOrigin, restoreFocus, type FocusOrigin } from '../focus-restore'
import { createKeyboardCues, cueRule, type KeyboardCues } from '../keyboard-cues'
import { createMenuStack, type MenuStack } from './menu-stack.svelte'
import { headerText, type MenuFlavor } from './rows'

export interface ContextMenuOpen {
  /** The rows to draw, as the backend answered them (`open_context_menu_in_page`). */
  rows: Row[]
  /** Where, in viewport pixels: the pointer, or the row's anchor for a keyboard-opened menu. */
  at: { x: number; y: number }
  /** Opened by Shift+F10 or the Menu key: the cursor starts on the first live row and the underlines show. */
  fromKeyboard: boolean
  /** What the menu was opened on: a scroll of anything holding it closes the menu, as a native one does. */
  anchorElement?: HTMLElement
  /** The menu's name, when it has no header line to take it from. */
  label?: string
  /**
   * The element that keeps focus while the menu is up (a text field's edit menu): the menu never
   * takes focus, so the field's blur handlers never run (an inline rename would cancel), and the
   * cursor reaches a screen reader as the field's `aria-activedescendant`.
   */
  focusHolder?: HTMLElement
  /**
   * The control that opened it (the main-menu button): a press on it is the control's own, which
   * toggles the menu, so the menu doesn't close itself on it only to be opened again.
   */
  opener?: HTMLElement
}

export interface ContextMenuDeps {
  /** A row was picked: its menu id. The menu is closed and focus is back where it was. */
  onPick: (menuId: string) => void
  /** Defaults to `desktop`: only Windows draws its context menus in the page. The demo passes `mac`. */
  flavor?: () => MenuFlavor
  /** Whether this platform has access keys (`convention('accessKeys')`). */
  accessKeys?: () => boolean
  /** Called whenever the menu closes: `picked` when a pick closed it, and `onPick` runs right after. */
  onClose?: (closed: { picked: boolean }) => void
}

export interface ContextMenuController {
  readonly stack: MenuStack
  readonly isOpen: boolean
  readonly cuesShown: boolean
  readonly flavor: MenuFlavor
  open(request: ContextMenuOpen): void
  /** Closes it without moving focus. */
  close(): void
  destroy(): void
}

/** `node` and everything above it, the document included: what a scroll of would carry it. */
function holdersOf(node: Node | undefined): readonly Node[] | null {
  if (!node) return null
  const holders: Node[] = []
  for (let at: Node | null = node; at; at = at.parentNode) holders.push(at)
  return holders
}

export function createContextMenu(deps: ContextMenuDeps): ContextMenuController {
  const flavor = (): MenuFlavor => deps.flavor?.() ?? 'desktop'
  const hasLetters = (): boolean => flavor() === 'desktop' && (deps.accessKeys?.() ?? cueRule() !== 'none')
  let cuesShown = $state(false)
  let cues: KeyboardCues | null = null
  let stopCues: (() => void) | null = null
  let origin: FocusOrigin | null = null
  /**
   * What holds what the menu was opened on, read at open: a scroll of any of them closes it. Read
   * then, ❗ not at the scroll: a virtual list may re-render the row while the menu is up, and a
   * detached row has no ancestors left.
   */
  let anchorHolders: readonly Node[] | null = null
  /** The control that opened the menu, whose presses are its own (`ContextMenuOpen.opener`). */
  let opener: HTMLElement | null = null

  const stack = createMenuStack({
    flavor,
    accessKeys: hasLetters,
    onPick: (row) => {
      close(true, true)
      deps.onPick(row.id)
    },
  })

  function close(restore: boolean, picked = false): void {
    if (!stack.isOpen) return
    // Where an element held focus throughout, there's nothing to give back, and a restore would
    // do harm: a field inside the window's focus home (the rename editor in the explorer) would
    // be sent to the home's list, ending the edit.
    const focusStayed = stack.focusHolder !== null
    stack.close()
    window.removeEventListener('keydown', onKey, true)
    window.removeEventListener('keyup', onKeyup, true)
    document.removeEventListener('pointerdown', onPointerDown, true)
    document.removeEventListener('scroll', onScroll, true)
    document.removeEventListener('compositionstart', onBlur, true)
    window.removeEventListener('blur', onBlur)
    window.removeEventListener('resize', onBlur)
    stopCues?.()
    cues?.destroy()
    cues = null
    stopCues = null
    cuesShown = false
    anchorHolders = null
    opener = null
    const target = origin
    origin = null
    if (restore && !focusStayed) restoreFocus(target)
    deps.onClose?.({ picked })
  }

  const keys: Partial<Record<string, () => void>> = {
    ArrowDown: () => {
      stack.walk(1)
    },
    ArrowUp: () => {
      stack.walk(-1)
    },
    ArrowRight: () => {
      stack.enterSubmenu()
    },
    ArrowLeft: () => {
      stack.backOut()
    },
    Home: () => {
      stack.edge(true)
    },
    End: () => {
      stack.edge(false)
    },
    Enter: () => {
      stack.pickHighlighted()
    },
    ' ': () => {
      stack.pickHighlighted()
    },
    Escape: () => {
      if (!stack.backOut()) close(true)
    },
    Tab: () => {
      close(true)
    },
  }

  /** One key while the menu is up. True when the menu acted on it. */
  function handleKey(event: KeyboardEvent): boolean {
    const action = keys[event.key]
    if (action) {
      action()
      return true
    }
    if (!hasLetters() || event.ctrlKey || event.metaKey || Array.from(event.key).length !== 1) return false
    return stack.typeAhead(event.key.toLowerCase())
  }

  const MODIFIERS: readonly string[] = ['Shift', 'Control', 'Alt', 'Meta', 'AltGraph']

  /**
   * Every key while the menu is up is the menu's, as an open native menu swallows them. Where a
   * field keeps focus, even a key the menu doesn't use is kept from it, or it would type behind the
   * menu.
   */
  function onKey(event: KeyboardEvent): void {
    cues?.keydown(event)
    const handled = !event.metaKey && !(flavor() === 'mac' && event.altKey) && handleKey(event)
    if (handled) {
      event.preventDefault()
      cues?.reveal()
    } else if (stack.focusHolder && !MODIFIERS.includes(event.key)) {
      event.preventDefault()
    }
    event.stopPropagation()
  }

  function onKeyup(event: KeyboardEvent): void {
    cues?.keyup(event)
  }

  function onPointerDown(event: PointerEvent): void {
    cues?.pointerdown()
    const target = event.target as Element | null
    if (target?.closest('[data-command-menu="context"]')) return
    if (target && opener?.contains(target)) return
    // A click elsewhere closes the menu and goes where it went.
    close(false)
  }

  function onScroll(event: Event): void {
    const scrolled = event.target
    if (scrolled instanceof Node && anchorHolders?.includes(scrolled)) close(false)
  }

  function onBlur(): void {
    close(false)
  }

  return {
    stack,
    get isOpen() {
      return stack.isOpen
    },
    get cuesShown() {
      return cuesShown
    },
    get flavor() {
      return flavor()
    },
    open(request) {
      close(false)
      origin = captureFocusOrigin()
      anchorHolders = holdersOf(request.anchorElement)
      opener = request.opener ?? null
      stack.open(
        request.rows,
        { kind: 'point', x: request.at.x, y: request.at.y },
        {
          fromKeyboard: request.fromKeyboard,
          label: request.label ?? headerText(request.rows),
          focusHolder: request.focusHolder,
        },
      )
      cues = createKeyboardCues({
        rule: hasLetters() ? cueRule() : 'none',
        openedByKeyboard: request.fromKeyboard,
      })
      stopCues = cues.onChange((shown) => {
        cuesShown = shown
      })
      cuesShown = cues.shown
      window.addEventListener('keydown', onKey, true)
      window.addEventListener('keyup', onKeyup, true)
      document.addEventListener('pointerdown', onPointerDown, true)
      document.addEventListener('scroll', onScroll, true)
      // An input method starting a composition in the field owns the keys from there on.
      document.addEventListener('compositionstart', onBlur, true)
      window.addEventListener('blur', onBlur)
      window.addEventListener('resize', onBlur)
    },
    close() {
      close(false)
    },
    destroy() {
      close(false)
      stack.destroy()
    },
  }
}
