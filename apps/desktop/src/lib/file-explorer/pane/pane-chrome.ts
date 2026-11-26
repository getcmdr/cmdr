/**
 * Pane chrome: each pane's tab bar, location bar (volume button, path segments, repo chip,
 * eject), and its list's sort header, or the error screen's action row in place of a list. Each is a `data-pane-chrome` group inside the pane's
 * `data-pane-chrome-scope` wrapper. The function key bar is global, not chrome.
 *
 * Keyboard users reach it through "Move focus to the pane controls" (`pane.focusControls`),
 * VoiceOver users also by moving the VO cursor there. While focus sits in chrome the focused
 * control keeps Enter, Space, the arrows, and Home/End (`paneChromeOwnsKey`); the explorer's
 * key dispatch stops forwarding to the pane, and Escape goes back to the list. `Tab` stays
 * `pane.switch` and every modifier shortcut still runs its command.
 *
 * Moving between controls is arrow-key focus, ❌ not a roving `tabindex`: `Tab` never walks
 * the page here, so `tabindex` would only matter to WebKit, which makes a button with an
 * EXPLICIT `tabindex` mouse-focusable on macOS (`HTMLFormControlElement::isMouseFocusable`,
 * WebKit main, source-read 2026-10-15). A click would then leave focus on chrome, and the
 * next arrow would walk the toolbar instead of moving the cursor.
 */

const GROUP_SELECTOR = '[data-pane-chrome]'
const SCOPE_SELECTOR = '[data-pane-chrome-scope]'
/** What a group's arrows walk: enabled buttons, plus anything opted in with a `tabindex` (the repo chip). */
const CONTROL_SELECTOR = 'button:not(:disabled), [tabindex]:not(button)'

/** The keys a focused chrome control keeps for itself, as `formatKeyCombo` spells them. */
const OWN_KEYS = new Set(['Enter', 'Space', '↑', '↓', '←', '→', 'Home', 'End'])

/** Each group's last focused control, so hopping back resumes where the user left it. */
const lastFocused = new WeakMap<Element, HTMLElement>()

function groupOf(target: EventTarget | null): HTMLElement | null {
  return target instanceof Element ? target.closest<HTMLElement>(GROUP_SELECTOR) : null
}

/** True when `target` sits in a pane's chrome. For events. */
export function isPaneChromeTarget(target: EventTarget | null): boolean {
  return groupOf(target) !== null
}

/** True when FOCUS sits in a pane's chrome. For keyboard decisions, beside `isTextInputFocused`. */
export function isPaneChromeFocused(): boolean {
  return isPaneChromeTarget(document.activeElement)
}

/** Whether the focused chrome control keeps `combo` (a `formatKeyCombo` string) from the command dispatcher. */
export function paneChromeOwnsKey(combo: string): boolean {
  return OWN_KEYS.has(combo) && isPaneChromeFocused()
}

function controlsOf(group: Element): HTMLElement[] {
  return Array.from(group.querySelectorAll<HTMLElement>(CONTROL_SELECTOR))
}

function focusControl(group: Element, control: HTMLElement): void {
  lastFocused.set(group, control)
  control.focus()
}

/** Focuses `group`'s first control (the location bar's volume button). False when it has none. */
export function focusPaneChrome(group: HTMLElement): boolean {
  const first = controlsOf(group).at(0)
  if (!first) return false
  focusControl(group, first)
  return true
}

/** Entering a group from a neighbor: where the user left it, else its selected tab, else its first control. */
function resumeGroup(group: Element): boolean {
  const controls = controlsOf(group)
  const remembered = lastFocused.get(group)
  const target =
    (remembered && controls.includes(remembered) ? remembered : undefined) ??
    controls.find((control) => control.getAttribute('aria-selected') === 'true') ??
    controls.at(0)
  if (!target) return false
  focusControl(group, target)
  return true
}

/** The groups of the pane `group` belongs to, top to bottom (DOM order: tab bar, location bar, sort header). */
function siblingGroups(group: Element): Element[] {
  const scope = group.closest(SCOPE_SELECTOR)
  return scope ? Array.from(scope.querySelectorAll(GROUP_SELECTOR)) : [group]
}

function hop(group: Element, step: 1 | -1): boolean {
  const groups = siblingGroups(group)
  for (let i = groups.indexOf(group) + step; i >= 0 && i < groups.length; i += step) {
    if (resumeGroup(groups[i])) return true
  }
  return false
}

/**
 * Moves focus for an unmodified arrow, Home, or End pressed on a chrome control: ←/→ walk
 * the group (no wrap), Home/End jump to its ends, ↑/↓ hop to the same pane's neighboring
 * group. True when focus moved, so the caller claims the key; false leaves it alone.
 */
export function movePaneChromeFocus(event: KeyboardEvent): boolean {
  if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return false
  const group = groupOf(event.target)
  if (!group || !(event.target instanceof HTMLElement)) return false
  // However focus got here (an arrow, the command, the VoiceOver cursor), hopping back resumes it.
  lastFocused.set(group, event.target)
  if (event.key === 'ArrowUp') return hop(group, -1)
  if (event.key === 'ArrowDown') return hop(group, 1)

  const controls = controlsOf(group)
  const at = controls.indexOf(event.target)
  const next = at === -1 ? -1 : walkTarget(event.key, at, controls.length)
  if (next < 0 || next >= controls.length || next === at) return false
  focusControl(group, controls[next])
  return true
}

/** Where ←/→/Home/End go from control `at` of `count`; -1 for any other key. */
function walkTarget(key: string, at: number, count: number): number {
  switch (key) {
    case 'ArrowLeft':
      return at - 1
    case 'ArrowRight':
      return at + 1
    case 'Home':
      return 0
    case 'End':
      return count - 1
    default:
      return -1
  }
}
