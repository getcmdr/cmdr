/**
 * The focus-model contract a screen reader needs from a composite widget (a file list, a
 * menu), checked the same way in the unit-test DOM and in the real webview.
 *
 * What it asserts, for a `root` that owns a cursor:
 *
 * 1. `document.activeElement` is `root` or inside it.
 * 2. The focused element carries `aria-activedescendant`, unless its list is empty (no
 *    `option` / `menuitem*` inside it to name).
 * 3. That id resolves to exactly ONE element in the whole document (two panes rendering the
 *    same row id make `getElementById` pick the wrong pane), inside the focused element or an
 *    element it `aria-owns`, with a role of `option` or `menuitem*` and a non-empty
 *    accessible name.
 * 4. Every live region the caller expects is already in `root` and live. A region inserted
 *    after its first message announces nothing, so call this right after mount.
 *
 * Names come from `dom-accessibility-api` (the accname algorithm Testing Library uses), so a
 * jsdom/happy-dom run and a WKWebView run agree on what "named" means.
 *
 * Usage (Vitest):
 *     import { assertFocusModel } from '#lib/test-focus-model.js'
 *     assertFocusModel(surface)
 *
 * Usage (Playwright): `expectFocusModel(tauriPage, selector, options)` in
 * `test/e2e-playwright/helpers/focus-model.ts` bundles this module into the page and runs
 * `focusModelViolations` there.
 *
 * ❗ Self-contained on purpose: no app imports, so the page bundle is this file plus
 * `dom-accessibility-api` and nothing else.
 */

import { computeAccessibleName, getRole } from 'dom-accessibility-api'

export interface FocusModelOptions {
  /**
   * Focus may sit on `root` itself with no active descendant: the explorer container, which
   * holds focus when the focused pane shows no list (an error, a connect
   * view). An `aria-activedescendant` the focused element does carry is still checked.
   */
  focusMayBeContainer?: boolean
  /** The cursor row must have a non-empty accessible name. Default true. */
  requireCursorName?: boolean
  /** Selectors, resolved inside `root`, of live regions that must already exist. */
  liveRegions?: string[]
}

const CURSOR_ROLES = new Set(['option', 'menuitem', 'menuitemcheckbox', 'menuitemradio'])
const LIVE_ROLES = new Set(['status', 'alert', 'log'])

/** A short, stable description of an element for a violation message. */
function describe(el: Element | null): string {
  if (!el) return 'null'
  if (el === el.ownerDocument.body) return '<body>'
  const id = el.id ? `#${el.id}` : ''
  const cls = el.classList.length ? `.${Array.from(el.classList).join('.')}` : ''
  const role = el.getAttribute('role')
  return `<${el.tagName.toLowerCase()}${id}${cls}${role ? ` role="${role}"` : ''}>`
}

/** True when `target` is inside `owner` or inside an element `owner` lists in `aria-owns`. */
function isOwnedBy(owner: Element, target: Element): boolean {
  if (owner.contains(target)) return true
  const owned = (owner.getAttribute('aria-owns') ?? '').split(/\s+/).filter(Boolean)
  return owned.some((id) => owner.ownerDocument.getElementById(id)?.contains(target) === true)
}

function hasCursorRows(el: Element): boolean {
  return (
    el.querySelector('[role="option"], [role="menuitem"], [role="menuitemcheckbox"], [role="menuitemradio"]') !== null
  )
}

function checkActiveDescendant(focused: Element, options: FocusModelOptions, out: string[]): void {
  const id = focused.getAttribute('aria-activedescendant') ?? ''
  if (id === '') {
    if (hasCursorRows(focused)) out.push(`${describe(focused)} has focus and rows, but no aria-activedescendant`)
    return
  }
  const doc = focused.ownerDocument
  const matches = doc.querySelectorAll(`[id="${id.replace(/["\\]/g, '\\$&')}"]`)
  if (matches.length !== 1) {
    out.push(
      `aria-activedescendant "${id}" matches ${String(matches.length)} elements in the document, not exactly one`,
    )
    if (matches.length === 0) return
  }
  const target = matches[0]
  if (!isOwnedBy(focused, target)) {
    out.push(
      `aria-activedescendant "${id}" names ${describe(target)}, which is neither inside nor owned by ${describe(focused)}`,
    )
  }
  const role = getRole(target) ?? ''
  if (!CURSOR_ROLES.has(role)) {
    out.push(`aria-activedescendant "${id}" names ${describe(target)} with role "${role}", not option or menuitem*`)
  }
  if (options.requireCursorName !== false && computeAccessibleName(target).trim() === '') {
    out.push(`aria-activedescendant "${id}" names ${describe(target)}, which has no accessible name`)
  }
}

function checkLiveRegions(root: Element, selectors: string[], out: string[]): void {
  for (const selector of selectors) {
    const regions = root.querySelectorAll(selector)
    if (regions.length !== 1) {
      out.push(
        `live region ${selector} matches ${String(regions.length)} elements in ${describe(root)}, not exactly one`,
      )
      continue
    }
    const region = regions[0]
    const live = region.getAttribute('aria-live')
    if (live !== 'polite' && live !== 'assertive' && !LIVE_ROLES.has(region.getAttribute('role') ?? '')) {
      out.push(`live region ${selector} (${describe(region)}) is not live (no aria-live or live role)`)
    }
  }
}

/** Every way `root` breaks the focus model; empty when it holds. */
export function focusModelViolations(root: Element | null, options: FocusModelOptions = {}): string[] {
  if (!root) return ['root is missing']
  const out: string[] = []
  const focused = root.ownerDocument.activeElement
  if (!focused || !root.contains(focused)) {
    out.push(`focus is on ${describe(focused)}, outside ${describe(root)}`)
  } else if (options.focusMayBeContainer && focused === root && !root.hasAttribute('aria-activedescendant')) {
    // The explorer's fallback: the container holds focus and names no row. Nothing to resolve.
  } else {
    checkActiveDescendant(focused, options, out)
  }
  checkLiveRegions(root, options.liveRegions ?? [], out)
  return out
}

/** Throws one error listing every violation, for unit tests. */
export function assertFocusModel(root: Element | null, options: FocusModelOptions = {}): void {
  const violations = focusModelViolations(root, options)
  if (violations.length > 0) throw new Error(`Focus model violated:\n- ${violations.join('\n- ')}`)
}
