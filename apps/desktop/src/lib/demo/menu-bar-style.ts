/**
 * The demo menus' look. Every visitor's bar and right-click menus are the shared component
 * (`#lib/ui/MenuBar.svelte`, `ContextMenu.svelte`) on `app-menu.css`'s tokens, which `data-look`
 * keys to the visitor's OS: a Windows visitor's wear Windows 11's as they are. A Mac visitor's get
 * macOS 27's here, on the one class the demo puts on their bar and menus ({@link DEMO_MENU_CLASS}),
 * since the desktop's macOS bar is AppKit's and has no tokens of its own: the app's own glass
 * tokens (`app.css` § Frosted-glass material), so they follow the Liquid Glass slider and
 * reduce-transparency like every other glass surface. Measured against the real bar and an open
 * menu on macOS 27 (`screencapture`, 2026-11-04): a 30 px bar, 13 px text, capsule title
 * highlights, 12 px menu corners, 24 px rows inset by 5 px, the accent pill under the cursor row,
 * and dimmed shortcut glyphs right-aligned with a little air between them. Injected once.
 */
import { getLook } from '#lib/platform/index.js'

/** The class on a Mac visitor's bar and every menu it opens, which carries the macOS look. */
export const DEMO_MENU_CLASS = 'cmdr-demo-macos-menu'

/** The class the demo's bar and menus wear: the macOS look for a Mac visitor, none (their OS's own tokens) for anyone else. */
export function demoMenuClass(): string {
  return getLook() === 'macos' ? DEMO_MENU_CLASS : ''
}

export const MENU_BAR_STYLE = `
.${DEMO_MENU_CLASS} {
  /* Surface */
  --spacing-menu-surface-min-width: 220px;
  --spacing-menu-surface-padding-block: 5px;
  /* Row: 24 px tall, 5 px in from the surface, the checkmark column 4 px in */
  --spacing-menu-row-margin-block: 0;
  --spacing-menu-row-margin-inline: 5px;
  --spacing-menu-row-padding-top: 0;
  --spacing-menu-row-padding-bottom: 0;
  --spacing-menu-row-padding-start: 4px;
  --spacing-menu-row-padding-end: 10px;
  --spacing-menu-row-min-height: 24px;
  --radius-menu-row: 7px;
  --font-menu-size: 13px;
  --color-menu-row-fg: var(--color-text-primary);
  /* The accent pill, white text on it */
  --color-menu-row-bg-hover: var(--color-system-accent);
  --color-menu-row-bg-focus: var(--color-system-accent);
  --color-menu-row-bg-pressed: var(--color-system-accent);
  --color-menu-row-bg-open: var(--color-system-accent);
  --color-menu-row-fg-hover: #fff;
  --color-menu-row-fg-focus: #fff;
  --color-menu-row-fg-open: #fff;
  --color-menu-row-fg-disabled: color-mix(in srgb, var(--color-text-tertiary) 55%, transparent);
  /* A 14 px checkmark cell and 4 px of air, in every menu */
  --spacing-menu-glyph-size: 14px;
  --spacing-menu-glyph-column: 18px;
  --spacing-menu-glyph-column-empty: 18px;
  --color-menu-check-fg: currentColor;
  /* Shortcut glyphs and the submenu arrow, 28 px off the label */
  --font-menu-shortcut-size: 13px;
  --spacing-menu-shortcut-gap: 28px;
  --color-menu-shortcut-fg: var(--color-text-tertiary);
  --color-menu-shortcut-fg-hover: rgb(255 255 255 / 0.85);
  --color-menu-shortcut-fg-focus: rgb(255 255 255 / 0.85);
  --color-menu-shortcut-fg-disabled: color-mix(in srgb, var(--color-text-tertiary) 55%, transparent);
  --color-menu-chevron-fg: var(--color-text-secondary);
  --color-menu-chevron-fg-hover: #fff;
  --color-menu-chevron-fg-focus: #fff;
  --color-menu-chevron-fg-pressed: #fff;
  /* A right-click menu's dimmed small header line (the desktop restyles that line the same way, \`context_menu_header.rs\`) */
  --font-menu-header-size: 11px;
  --color-menu-header-fg: var(--color-text-secondary);
  --spacing-menu-header-min-height: 22px;
  --color-menu-separator: var(--color-border-glass);
  --spacing-menu-separator-margin-block: 5px;
  --spacing-menu-separator-inset: 15px;
  /* A Mac bar's title lights only while its menu is open or the keyboard is on it */
  --color-menu-bar-item-bg-hover: transparent;
}
/* The app menu's title, the first, bold as macOS draws it */
.${DEMO_MENU_CLASS} .menu-bar-title:first-child { font-weight: 700; }
.${DEMO_MENU_CLASS}.menu-bar {
  position: relative;
  z-index: calc(var(--z-modal-top) + 1);
  /* No backdrop-filter on the bar: it sits at the page's top edge with nothing behind it to blur,
     and the blur mixes in the transparency past two edges at its corners, which then show through
     as dark notches (most visibly in the home page's framed hero). The menus keep theirs. */
  border-bottom: 0.5px solid var(--color-border-glass);
}
/* The app's two corners that sit against the page's top edge (the status corner resolves against
   the initial containing block, the toast stack is fixed): on the desktop that edge is the
   window's, here it's the bar's, so both move down by the bar or the bar covers them and eats
   their clicks. The same offsets as their own rules, plus the bar. \`html:root\` outranks the
   components' scoped classes. */
html:root .status-corner {
  top: calc(var(--cmdr-demo-window-top, 0px) + var(--spacing-sm) - var(--spacing-xxs));
}
html:root .toast-container:not(.at-bottom) {
  top: max(calc(var(--cmdr-demo-window-top, 0px) + var(--spacing-lg)), calc(var(--titlebar-status-corner-bottom) + var(--spacing-sm)));
}
`

const STYLE_ID = 'cmdr-demo-menu-style'

/** Injects the menus' look into this document once, for whichever of the bar and the right-click menus comes first. Answers the teardown. */
export function ensureMenuStyle(): () => void {
  if (document.getElementById(STYLE_ID)) return () => {}
  const style = document.createElement('style')
  style.id = STYLE_ID
  style.textContent = MENU_BAR_STYLE
  document.head.append(style)
  return () => {
    style.remove()
  }
}
