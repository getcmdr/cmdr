/**
 * The in-page menu bar's behavior (`MenuBar.svelte` draws it): which title has the keyboard,
 * which menu is open, the keys, and where focus goes back to. Its flyouts are a `menu-stack`.
 *
 * Keys, by flavor (`rows.ts` § `MenuFlavor`):
 * - `desktop` (Windows, KDE, other Linux): a lone Alt tap or F10 takes the bar and gives it back,
 *   Alt plus a title's letter opens that menu, a letter in an open menu picks its row. A lone Alt
 *   doesn't count once another key came between press and release, AltGr never does, and
 *   Alt+Space, Alt+F4, and Shift+F10 stay the system's.
 * - `mac` (the browser demo's macOS bar): ⌃F2 takes the bar, as macOS's "Move focus to the menu
 *   bar" does.
 *
 * Then, both: ←/→ move along the bar (opening the neighbor when a menu is open), ↓ / Enter / Space
 * open, ↑/↓ walk, → enters a submenu and ← leaves it, Home/End jump, Esc backs out one level and
 * then gives the keyboard back to where it was, and Tab leaves. Every key goes to the bar while it
 * has the keyboard, never the panes behind it. Mechanics: `../DETAILS.md` § Command menus.
 */

import type { BarMenuView } from '#lib/ipc/bindings.js'
import { allocateAccessKeys, accessKeyLettersOf } from '../access-keys'
import { captureFocusOrigin, isInFocusHome, restoreFocus, type FocusOrigin } from '../focus-restore'
import { createKeyboardCues, cueRule, type KeyboardCues } from '../keyboard-cues'
import { isAltGraphHeld } from '#lib/shortcuts/key-capture.js'
import { isTextInputFocused } from '#lib/utils/text-input-focus.js'
import { createMenuStack, type MenuStack } from './menu-stack.svelte'
import { rowLabelText, type MenuFlavor } from './rows'

export interface MenuBarDeps {
  getMenus: () => readonly BarMenuView[]
  /** A row was picked: its menu id. Focus is already back where it was. */
  onPick: (menuId: string) => void
  flavor: () => MenuFlavor
  /** Whether this platform has access keys (`convention('accessKeys')`). */
  accessKeys: () => boolean
  /** Letters the window's own Alt+letter commands hold (the viewer's `Alt+R`), which no title gets. */
  reservedLetters: () => ReadonlySet<string>
  /** True while a modal surface is up: the bar takes no keys and no clicks. */
  isInert: () => boolean
  /**
   * Whether the window's own keys reach this bar (Alt, F10, Alt+letter, ⌃F2). True for a window's
   * one bar; false for a second bar on the same page (the Debug catalog's), which still answers
   * its own keys once a click hands it the keyboard.
   */
  takesWindowKeys: () => boolean
  /** The bar's element, and a title's, for containment and placement. */
  barElement: () => HTMLElement | undefined
  titleElement: (index: number) => HTMLElement | undefined
}

export interface MenuBarController {
  readonly stack: MenuStack
  /** The title that holds the roving tab stop. */
  readonly focusIndex: number
  readonly openIndex: number | null
  /** Whether the bar has the keyboard (its title or its menu). */
  readonly active: boolean
  /** Whether access-key underlines show now (`keyboard-cues.ts`). */
  readonly cuesShown: boolean
  /**
   * The element that held focus when the bar took the keyboard, while it has it; `null` otherwise.
   * The bar moves focus into itself, so a page whose rows follow a field's focus (the viewer's Cut
   * and Paste follow its search box) reads this to keep them live while its menu is open.
   */
  readonly focusOrigin: Element | null
  /**
   * The text field that keeps DOM focus while the bar has the keyboard, or `null` where the bar
   * holds focus itself. A field the bar took the keyboard from never blurs: the keyboard's title
   * and the open menu's cursor reach a screen reader as its `aria-activedescendant`, so a field
   * that ends its edit on blur (the inline rename) survives a pick from the bar's Edit menu.
   */
  readonly fieldHolder: HTMLElement | null
  /** Each title's access key, or null. */
  readonly titleLetters: readonly (string | null)[]
  pointerDownTitle(index: number, event: PointerEvent): void
  pointerEnterTitle(index: number): void
  /** The menus changed under an open one: redraw it in place. */
  refresh(): void
  /** Closes everything without moving focus. */
  close(): void
  /** Starts listening to the window; returns the teardown. */
  attach(): () => void
}

/** Where a flyout hangs under its title: the bar's bottom edge, at the title's left, nudged as each flavor's own does. */
const FLYOUT_NUDGE: Record<MenuFlavor, { x: number; y: number }> = {
  // The demo's measured macOS 27 bar: 2 px left of the title's capsule, 1 px under the bar.
  mac: { x: -2, y: 1 },
  // WinUI's `MenuBarItem` opens its flyout at the item's bottom-left (`BottomEdgeAlignedLeft`).
  desktop: { x: 0, y: 0 },
}

const MODIFIER_KEYS: readonly string[] = ['Shift', 'Control', 'Alt', 'Meta', 'AltGraph']

function isLoneAlt(event: KeyboardEvent): boolean {
  return event.key === 'Alt' && !event.ctrlKey && !event.metaKey && !event.shiftKey && !isAltGraphHeld(event)
}

function noModifiers(event: KeyboardEvent): boolean {
  return !event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey
}

export function createMenuBar(deps: MenuBarDeps): MenuBarController {
  let focusIndex = $state(0)
  let openIndex = $state<number | null>(null)
  let active = $state(false)
  let cuesShown = $state(false)
  let origin: FocusOrigin | null = null
  let focusOrigin = $state<Element | null>(null)
  let fieldHolder = $state<HTMLElement | null>(null)
  /** A lone Alt is down and nothing else came since: its release takes or gives back the bar. */
  let altArmed = false
  /** The bar took focus back from the app once this session (`onFocusIn`), so a second re-home isn't fought. */
  let reclaimedFocus = false

  const hasLetters = (): boolean => deps.flavor() === 'desktop' && deps.accessKeys()
  const rule = () => (hasLetters() ? cueRule() : 'none')

  /** The bar at rest: underlined while Alt is held (and always under Windows' setting). */
  let idleCues: KeyboardCues = createKeyboardCues({ rule: 'none' })
  /** One per time the bar takes the keyboard, by the platform's rule. */
  let sessionCues: KeyboardCues | null = null
  let stopSession: (() => void) | null = null

  function syncCues(): void {
    cuesShown = active && sessionCues ? sessionCues.shown : idleCues.shown
  }

  const stack = createMenuStack({
    flavor: deps.flavor,
    accessKeys: hasLetters,
    onPick: (row) => {
      deactivate(true)
      deps.onPick(row.id)
    },
  })

  const titleLetters = $derived.by(() => {
    if (!hasLetters()) return deps.getMenus().map(() => null)
    return allocateAccessKeys(
      deps.getMenus().map((menu) => rowLabelText(menu.title)),
      deps.reservedLetters(),
    )
  })

  function count(): number {
    return deps.getMenus().length
  }

  function wrap(index: number): number {
    const n = count()
    return n === 0 ? 0 : ((index % n) + n) % n
  }

  function activate(fromKeyboard: boolean): void {
    if (active) return
    origin = captureFocusOrigin()
    focusOrigin = document.activeElement
    // Regression anchor (review of #1271): a menu taking focus from the inline rename's field
    // ended the rename, so the Edit rows it opened for had nothing left to act on.
    fieldHolder = isTextInputFocused() && focusOrigin instanceof HTMLElement ? focusOrigin : null
    active = true
    reclaimedFocus = false
    sessionCues = createKeyboardCues({ rule: rule(), openedByKeyboard: fromKeyboard })
    stopSession = sessionCues.onChange(syncCues)
    syncCues()
  }

  /** Gives the keyboard back: to where it was (`restore`), or nowhere, as a click elsewhere does. */
  function deactivate(restore: boolean): void {
    stack.close()
    openIndex = null
    if (!active) return
    active = false
    stopSession?.()
    sessionCues?.destroy()
    sessionCues = null
    stopSession = null
    syncCues()
    const leaving = deps.barElement()
    const target = origin
    const focusStayed = fieldHolder !== null
    origin = null
    focusOrigin = null
    fieldHolder = null
    // Where the field kept focus, there's nothing to give back, and a restore would do harm: a
    // field inside the window's focus home (the rename editor) would be sent to the home's list.
    if (focusStayed) return
    if (restore) {
      restoreFocus(target, { leaving })
    } else if (leaving?.contains(document.activeElement)) {
      // A title keeping focus after the bar let go would leave every key with nowhere to go.
      ;(document.activeElement as HTMLElement | null)?.blur()
    }
  }

  function focusTitle(index: number): void {
    focusIndex = wrap(index)
  }

  function openMenu(index: number, fromKeyboard: boolean, fromEnd = false): void {
    const menu = deps.getMenus().at(wrap(index))
    const title = deps.titleElement(wrap(index))
    const bar = deps.barElement()
    if (!menu || !title || !bar) return
    const titleRect = title.getBoundingClientRect()
    const barRect = bar.getBoundingClientRect()
    const nudge = FLYOUT_NUDGE[deps.flavor()]
    const left = titleRect.left + nudge.x
    const bottom = barRect.bottom + nudge.y
    focusIndex = wrap(index)
    openIndex = wrap(index)
    stack.open(
      menu.rows,
      { kind: 'below', rect: { left, right: left, top: bottom, bottom } },
      { fromKeyboard, fromEnd, labelledBy: titleId(bar.id, wrap(index)), focusHolder: fieldHolder ?? undefined },
    )
  }

  function closeMenuKeepingTitle(): void {
    stack.close()
    openIndex = null
  }

  function toggle(): void {
    if (active) {
      deactivate(true)
      return
    }
    activate(true)
    focusTitle(0)
  }

  function titleIndexFor(letters: readonly string[]): number {
    for (const letter of letters) {
      const index = titleLetters.indexOf(letter)
      if (index >= 0) return index
    }
    return -1
  }

  /** ← and → along the bar: the neighbor menu opens while one is open, else the neighbor title takes focus. */
  function alongBar(step: 1 | -1): void {
    if (openIndex === null) focusTitle(focusIndex + step)
    else openMenu(openIndex + step, true)
  }

  /**
   * The bar's keys while it has the keyboard, each one flat call: one switch deciding all of them
   * ran far past the complexity cap. Each answers whether it acted.
   */
  const activeKeys: Partial<Record<string, () => void>> = {
    ArrowDown: () => {
      if (openIndex === null) openMenu(focusIndex, true)
      else stack.walk(1)
    },
    ArrowUp: () => {
      // Windows opens a menu from ↑ on its last row; the Mac on its first, as ↓ does.
      if (openIndex === null) openMenu(focusIndex, true, deps.flavor() === 'desktop')
      else stack.walk(-1)
    },
    ArrowRight: () => {
      if (openIndex === null || !stack.enterSubmenu()) alongBar(1)
    },
    ArrowLeft: () => {
      if (openIndex === null || !stack.backOut()) alongBar(-1)
    },
    Home: () => {
      if (openIndex === null) focusTitle(0)
      else stack.edge(true)
    },
    End: () => {
      if (openIndex === null) focusTitle(count() - 1)
      else stack.edge(false)
    },
    Enter: () => {
      if (openIndex === null) openMenu(focusIndex, true)
      else stack.pickHighlighted()
    },
    Escape: () => {
      if (stack.backOut()) return
      if (openIndex === null) deactivate(true)
      else closeMenuKeepingTitle()
    },
    Tab: () => {
      deactivate(true)
    },
  }
  activeKeys[' '] = activeKeys.Enter

  /** A letter with the bar active: the open menu's row, or the title, that it underlines. */
  function typeAhead(event: KeyboardEvent): boolean {
    if (!hasLetters() || !noModifiers(event) || Array.from(event.key).length !== 1) return false
    const letter = event.key.toLowerCase()
    if (openIndex !== null) return stack.typeAhead(letter)
    const index = titleLetters.indexOf(letter)
    if (index >= 0) openMenu(index, true)
    return index >= 0
  }

  /** The keys while the bar has the keyboard. True when the key was the bar's to act on. */
  function handleActiveKey(event: KeyboardEvent): boolean {
    const action = activeKeys[event.key]
    if (!action) return typeAhead(event)
    action()
    return true
  }

  /** Alt+a title's letter, from anywhere in the window. True when a title answered it. */
  function handleTitleAccessKey(event: KeyboardEvent): boolean {
    if (!hasLetters()) return false
    const index = titleIndexFor(accessKeyLettersOf(event))
    if (index < 0) return false
    activate(true)
    openMenu(index, true)
    return true
  }

  /**
   * The keys that reach the bar from anywhere in the window: Alt, F10, and Alt+letter on the
   * desktop flavor, ⌃F2 on the Mac. True when the key was the bar's and is done with.
   */
  function handleWindowKey(event: KeyboardEvent): boolean {
    if (deps.flavor() === 'mac') {
      if (event.key !== 'F2' || !event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return false
      activate(true)
      focusTitle(0)
      return true
    }
    if (isLoneAlt(event)) {
      if (!event.repeat) altArmed = true
      // A lone Alt goes on to the page, which must still see its keyup (task 63 step 6's guard).
      return false
    }
    altArmed = false
    if (event.key === 'F10' && noModifiers(event)) {
      toggle()
      return true
    }
    return event.altKey && handleTitleAccessKey(event)
  }

  function onKeydown(event: KeyboardEvent): void {
    if (deps.isInert() || event.isComposing) return
    idleCues.keydown(event)
    sessionCues?.keydown(event)
    if (deps.takesWindowKeys() && handleWindowKey(event)) {
      event.preventDefault()
      event.stopPropagation()
      sessionCues?.reveal()
      return
    }
    if (!active) return
    // The Mac bar leaves ⌘ and ⌥ keys to the app, as an open native menu does; the desktop bar ⌘ ones.
    if (event.metaKey || (deps.flavor() === 'mac' && event.altKey)) return
    if (handleActiveKey(event)) {
      event.preventDefault()
      sessionCues?.reveal()
    } else if (fieldHolder && !MODIFIER_KEYS.includes(event.key)) {
      // The field keeps focus, so a key the bar doesn't use would type into it behind the menu.
      event.preventDefault()
    }
    // Whatever the bar didn't use stays out of the app, as an open native menu swallows it.
    event.stopPropagation()
  }

  function onKeyup(event: KeyboardEvent): void {
    idleCues.keyup(event)
    sessionCues?.keyup(event)
    if (event.key !== 'Alt' || !altArmed) return
    altArmed = false
    if (deps.isInert() || !deps.takesWindowKeys()) return
    event.preventDefault()
    toggle()
  }

  /** Whether `node` is the bar's or one of its menus'. */
  function owns(node: Node): boolean {
    if (deps.barElement()?.contains(node)) return true
    const el = node instanceof Element ? node : node.parentElement
    return el?.closest('[data-command-menu="bar"]') != null
  }

  function onPointerDown(event: PointerEvent): void {
    altArmed = false
    idleCues.pointerdown()
    sessionCues?.pointerdown()
    const target = event.target as Node | null
    if (!active || (target && owns(target))) return
    // A click elsewhere closes the bar and goes where it went.
    deactivate(false)
  }

  function onBlur(): void {
    // WebView2 sends no Alt keyup when the window loses focus mid-press, so the Alt state resets.
    altArmed = false
    idleCues.blur()
    sessionCues?.blur()
    deactivate(false)
  }

  /**
   * Focus moved on by other means while the bar has the keyboard. It lets go, as Win32's does,
   * only when focus leaves something the bar owns for somewhere new (a text field a ⌘ command
   * opened). Two moves aren't the person's, so they leave it be:
   * - Focus that never came from the bar: a click on a title with focus on `<body>`, then the
   *   explorer re-homing it to its list (`DualPaneExplorer`'s refocus effect).
   * - Focus the app puts back in its home (the explorer's lists): the person's keys are the
   *   bar's and their clicks close it on `pointerdown`, so that's the app re-homing, which on a
   *   fresh page lands right after the first ⌃F2's title took focus. The bar takes it back once.
   */
  function onFocusIn(event: FocusEvent): void {
    const target = event.target as Node | null
    const from = event.relatedTarget as Node | null
    if (!active || !target || owns(target) || !from || !owns(from)) return
    if (!isInFocusHome(target)) {
      deactivate(false)
      return
    }
    if (reclaimedFocus) return
    reclaimedFocus = true
    queueMicrotask(reclaimFocus)
  }

  /** Puts focus back on the keyboard's title, or the open menu's surface, while the bar still has the keyboard. */
  function reclaimFocus(): void {
    if (!active || fieldHolder) return
    if (openIndex === null) {
      deps.titleElement(focusIndex)?.focus({ preventScroll: true })
      return
    }
    // The surface the keyboard is in holds focus: its cursor's row, or itself (`MenuSurface.svelte`).
    const surface = document.getElementById(stack.surfaces.at(stack.activeDepth)?.id ?? '')
    const target = surface?.querySelector<HTMLElement>('[tabindex="0"]') ?? surface
    target?.focus({ preventScroll: true })
  }

  return {
    stack,
    get focusIndex() {
      return focusIndex
    },
    get openIndex() {
      return openIndex
    },
    get active() {
      return active
    },
    get focusOrigin() {
      return focusOrigin
    },
    get fieldHolder() {
      return fieldHolder
    },
    get cuesShown() {
      return cuesShown
    },
    get titleLetters() {
      return titleLetters
    },
    pointerDownTitle(index, event) {
      if (event.button !== 0 || deps.isInert()) return
      // Focus stays where it was until the menu takes it, so Esc can give it back.
      event.preventDefault()
      if (openIndex === index) {
        deactivate(true)
        return
      }
      activate(false)
      openMenu(index, false)
    },
    pointerEnterTitle(index) {
      if (openIndex !== null && openIndex !== index) openMenu(index, false)
    },
    refresh() {
      if (openIndex === null) return
      const menu = deps.getMenus().at(openIndex)
      if (menu) stack.refresh(menu.rows)
      else closeMenuKeepingTitle()
    },
    close() {
      deactivate(false)
    },
    attach() {
      idleCues.destroy()
      idleCues = createKeyboardCues({ rule: rule() === 'none' ? 'none' : 'whileAltHeld' })
      const stopIdle = idleCues.onChange(syncCues)
      syncCues()
      window.addEventListener('keydown', onKeydown, true)
      window.addEventListener('keyup', onKeyup, true)
      window.addEventListener('blur', onBlur)
      document.addEventListener('pointerdown', onPointerDown, true)
      document.addEventListener('focusin', onFocusIn)
      return () => {
        deactivate(false)
        stack.destroy()
        stopIdle()
        idleCues.destroy()
        window.removeEventListener('keydown', onKeydown, true)
        window.removeEventListener('keyup', onKeyup, true)
        window.removeEventListener('blur', onBlur)
        document.removeEventListener('pointerdown', onPointerDown, true)
        document.removeEventListener('focusin', onFocusIn)
      }
    },
  }
}

/** A title's element id, unique per bar, which names its open menu. */
export function titleId(barId: string, index: number): string {
  return `${barId}-title-${String(index)}`
}
