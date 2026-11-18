/**
 * A window's in-page right-click menu, where `convention('contextMenus')` is `inPage` (Windows):
 * one `createContextMenu` controller (`#lib/ui/ContextMenu.svelte` draws it), opened either by a
 * `show_*` wrapper through `popContextMenu` (the backend's rows, picked through
 * `pickContextMenuItem`) or by a right-click in a text field (`text-field-menu.ts`, acting on the
 * field itself). Mechanics: `DETAILS.md`.
 *
 * Where a menu opens: the anchor the caller named (a keyboard-opened file menu, the date chevron),
 * else the last right-click's point. Whether it opened from the keyboard is what the last input was
 * (a key, or a pointer press), so the cursor starts on the first row and the underlines show only
 * when a key opened it.
 */

import type { InPageContextMenu, Row } from '#lib/ipc/bindings.js'
import { getAppLogger } from '#lib/logging/logger.js'
import { pickContextMenuItem } from '#lib/tauri-commands/index.js'
import { createContextMenu, type ContextMenuController } from '#lib/ui/command-menu/context-menu.svelte.js'

const log = getAppLogger('contextMenu')

/** A point in viewport CSS pixels. */
export interface MenuPoint {
  x: number
  y: number
}

/** One menu up: what a pick of each row does, and what closing it settles. */
export interface MenuSession {
  rows: Row[]
  /** Where it opens: `null` for the last right-click's point. */
  at: MenuPoint | null
  /** What it was opened on, when the caller knows better than the last right-click's target. */
  anchorElement?: HTMLElement
  label?: string
  /** The element that keeps focus while it's up (`ContextMenuOpen.focusHolder`): a text field. */
  focusHolder?: HTMLElement
  /** A row was picked (the menu is closed and focus is back): resolves once its action ran. */
  onPick: (menuId: string) => Promise<void>
  /** The menu is gone and its pick, if any, has run. */
  onSettled?: () => void
}

export interface ContextMenuHost {
  readonly menu: ContextMenuController
  /** Opens `session`'s rows, closing (and settling) whatever menu was up. */
  open(session: MenuSession): void
  /**
   * Draws a backend-answered menu (`popContextMenu`'s drawer); resolves once it's closed and its
   * pick ran. An answer asked before a menu of the window's own opened (a text field's) is dropped.
   */
  draw(answer: InPageContextMenu, askedAt: number): Promise<void>
  /** An input method is composing: a right-click then is the input method's, so no menu opens. */
  readonly isComposing: boolean
  /** Starts following the window's input (where the last right-click landed, keyboard or pointer). */
  install(): () => void
}

/** Opened from the keyboard: Shift+F10, the Menu key, or Enter on a chevron. */
type InputKind = 'keyboard' | 'pointer'

/**
 * Whether the keyboard raised this `contextmenu` (Shift+F10 or the Menu key in a text field). Chromium
 * (WebView2) fires it as a `PointerEvent` whose `pointerType` is empty for a key and `mouse`, `pen`,
 * or `touch` otherwise; an engine firing a plain `MouseEvent` gives a key's no pressed button
 * (unverified on WebView2 itself: Chromium's documented behavior, 2026-11-18).
 */
export function isKeyboardContextMenu(event: MouseEvent): boolean {
  if (typeof PointerEvent !== 'undefined' && event instanceof PointerEvent) return event.pointerType === ''
  return event.button !== 2
}

export function createContextMenuHost(): ContextMenuHost {
  let session: MenuSession | null = null
  let lastInput: InputKind = 'pointer'
  let lastPoint: MenuPoint | null = null
  let lastTarget: HTMLElement | null = null
  /** When `open` last ran for a menu this window drew on its own (`performance.now()`). */
  let lastOwnOpenAt = -Infinity
  let composing = false

  function settle(done: MenuSession | null): void {
    done?.onSettled?.()
  }

  const menu = createContextMenu({
    onPick: (menuId) => {
      const picked = session
      session = null
      if (!picked) return
      picked
        .onPick(menuId)
        .catch((error: unknown) => {
          log.warn("Couldn't run the right-click menu's {menuId} pick: {error}", { menuId, error })
        })
        .finally(() => {
          settle(picked)
        })
    },
    onClose: ({ picked }) => {
      if (picked) return
      const closed = session
      session = null
      settle(closed)
    },
  })

  /** Where focus is, for a menu with no point: just under the focused element's left edge. */
  function focusPoint(): MenuPoint {
    const active = document.activeElement
    if (active instanceof HTMLElement && active !== document.body) {
      const rect = active.getBoundingClientRect()
      return { x: rect.left, y: rect.bottom }
    }
    return { x: 0, y: 0 }
  }

  function openSession(next: MenuSession): void {
    // Opening closes the menu that was up, which settles it through `onClose`.
    menu.close()
    const fromKeyboard = lastInput === 'keyboard'
    const at = next.at ?? (fromKeyboard ? null : lastPoint) ?? focusPoint()
    const active = document.activeElement
    const anchorElement =
      next.anchorElement ??
      (fromKeyboard ? (active instanceof HTMLElement ? active : undefined) : (lastTarget ?? undefined))
    session = next
    menu.open({ rows: next.rows, at, fromKeyboard, anchorElement, label: next.label, focusHolder: next.focusHolder })
  }

  function draw(answer: InPageContextMenu, askedAt: number): Promise<void> {
    if (lastOwnOpenAt > askedAt) return Promise.resolve()
    return new Promise((resolve) => {
      openSession({
        rows: answer.rows,
        at: answer.anchor,
        onPick: async (menuId) => {
          const outcome = await pickContextMenuItem(answer.token, menuId)
          if (outcome === 'stale') log.debug('A pick from a right-click menu another one replaced ran nothing')
        },
        onSettled: resolve,
      })
    })
  }

  function onContextMenu(event: MouseEvent): void {
    if (isKeyboardContextMenu(event)) {
      lastInput = 'keyboard'
      return
    }
    lastInput = 'pointer'
    lastPoint = { x: event.clientX, y: event.clientY }
    lastTarget = event.target instanceof HTMLElement ? event.target : null
  }

  function onCompositionStart(): void {
    composing = true
  }

  function onCompositionEnd(): void {
    composing = false
  }

  function onPointerDown(): void {
    lastInput = 'pointer'
  }

  function onKeyDown(event: KeyboardEvent): void {
    // A lone modifier says nothing about how the next menu opens (Shift of Shift+F10 included).
    if (event.key === 'Shift' || event.key === 'Control' || event.key === 'Alt' || event.key === 'Meta') return
    lastInput = 'keyboard'
  }

  return {
    menu,
    open(next) {
      lastOwnOpenAt = performance.now()
      openSession(next)
    },
    draw,
    get isComposing() {
      return composing
    },
    install() {
      document.addEventListener('contextmenu', onContextMenu, true)
      document.addEventListener('pointerdown', onPointerDown, true)
      document.addEventListener('keydown', onKeyDown, true)
      document.addEventListener('compositionstart', onCompositionStart, true)
      document.addEventListener('compositionend', onCompositionEnd, true)
      return () => {
        document.removeEventListener('contextmenu', onContextMenu, true)
        document.removeEventListener('pointerdown', onPointerDown, true)
        document.removeEventListener('keydown', onKeyDown, true)
        document.removeEventListener('compositionstart', onCompositionStart, true)
        document.removeEventListener('compositionend', onCompositionEnd, true)
      }
    },
  }
}
