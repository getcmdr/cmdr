/**
 * The demo's Windows and Linux title bars: the OS-drawn strip a desktop Cmdr window has on those
 * OSes, drawn by the page because no OS draws it. The desktop app keeps the OS's own decorations
 * there (no `titleBarStyle` off macOS, so `overlayTitleBar` is off and no page leaves room under
 * lights), so each window is the OS's title bar (its title and minimize, maximize, and close)
 * with the page under it, and on the main window the in-window menu bar under that. A Mac visitor
 * gets `traffic-lights.ts` instead. Windows 11's caption metrics (a 32 px bar, 46 × 32 px buttons,
 * the red close hover) and a desktop-neutral Linux bar (a page can't tell GNOME from KDE: the
 * title centered, three round buttons on the right). A draft David reviews. See `DETAILS.md` §
 * Window buttons.
 */
import tauriConfig from '../../../src-tauri/tauri.conf.json'
import { getMessage } from '#lib/intl/messages.svelte.js'
import type { Platform } from '#lib/platform/index.js'
import { canFullScreen, isBrowserFullScreen, setBrowserFullScreen } from './browser-full-screen.js'
import { followLanguage } from './follow-language.svelte.js'
import type { ZoomGlyph } from './traffic-lights.js'

/** The two OSes whose windows carry an OS title bar above the page. */
export type TitleBarLook = Exclude<Platform, 'macos'>

/** What one window's buttons do. A missing action leaves that button inert (`aria-disabled`). */
export interface TitleBarActions {
  close?: () => void
  minimize?: () => void
  maximize?: () => void
  /** A press on the bar itself (not a button), as the OS starts moving the window. */
  drag?: (event: MouseEvent) => void
}

export interface TitleBar {
  root: HTMLElement
  /** Dimmed when the window isn't the active one, as both OSes draw an inactive window's bar. */
  setActive(active: boolean): void
  /** Maximize's glyph: maximize for a window that isn't (`zoom`, `enterFullScreen`), restore for one that is. */
  setZoomGlyph(glyph: ZoomGlyph): void
  /** Stops following the UI language, for a window that closed. */
  destroy(): void
}

/** Each look's bar height, in CSS pixels: Windows 11's caption at 100%, and a GTK title bar's. */
export const TITLE_BAR_HEIGHT: Readonly<Record<TitleBarLook, number>> = { windows: 32, linux: 37 }

/** The main window's title, from the same `tauri.conf.json` the Windows and Linux apps build it from. */
export const MAIN_WINDOW_TITLE: string = tauriConfig.app.windows[0].title

/** The custom property the main window's bar height lands on, so the demo's other chrome starts under it. */
export const TITLE_BAR_HEIGHT_PROPERTY = '--cmdr-demo-title-bar-height'

/** A glyph as a CSS mask, in a 10-unit box (Segoe Fluent Icons' caption glyphs are 10 px). */
function glyph(shapes: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="#000" stroke-width="1">${shapes}</svg>`
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}")`
}

const GLYPHS = {
  minimize: glyph('<path d="M0 5.5h10"/>'),
  maximize: glyph('<rect x="0.5" y="0.5" width="9" height="9" rx="1"/>'),
  restore: glyph(
    '<rect x="0.5" y="2.5" width="7" height="7" rx="1"/><path d="M2.5 2.5v-1a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1"/>',
  ),
  close: glyph('<path d="M0.5 0.5l9 9M9.5 0.5l-9 9"/>'),
}

const STYLE = `
.cmdr-demo-titlebar {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  position: relative;
  /* First in the page's column, above the in-window menu bar, as the OS's caption sits above the
     window's own content. */
  order: -1;
  font-family: var(--font-system);
  color: var(--t-fg);
  background: var(--t-bg);
  cursor: default;
  -webkit-user-select: none;
  user-select: none;
}
.cmdr-demo-titlebar[data-active='false'] { color: var(--t-fg-inactive); }
.cmdr-demo-titlebar-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  pointer-events: none;
}
.cmdr-demo-titlebar-buttons { display: flex; align-self: stretch; align-items: center; }
.cmdr-demo-titlebar-button {
  position: relative;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  cursor: default;
  -webkit-tap-highlight-color: transparent;
}
.cmdr-demo-titlebar-button::after {
  content: '';
  position: absolute;
  inset: 0;
  background: currentColor;
  -webkit-mask: var(--t-glyph) center / 10px 10px no-repeat;
  mask: var(--t-glyph) center / 10px 10px no-repeat;
}
.cmdr-demo-titlebar-button[aria-disabled='true']::after { opacity: 0.4; }
.cmdr-demo-titlebar-button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; }
.cmdr-demo-titlebar-button[data-button='minimize'] { --t-glyph: ${GLYPHS.minimize}; }
.cmdr-demo-titlebar-button[data-button='maximize'] { --t-glyph: ${GLYPHS.maximize}; }
.cmdr-demo-titlebar[data-zoom='exitFullScreen'] [data-button='maximize'] { --t-glyph: ${GLYPHS.restore}; }
.cmdr-demo-titlebar-button[data-button='close'] { --t-glyph: ${GLYPHS.close}; }

/* Windows 11: the title at the left in 12 px, the buttons flush right, 46 × 32 each; a hover
   tints the button, close's goes red with a white glyph (sampled from Windows 11 24H2's caption
   buttons, light and dark). */
.cmdr-demo-titlebar[data-look='windows'] {
  height: ${String(TITLE_BAR_HEIGHT.windows)}px;
  font-size: 12px;
  --t-bg: #f3f3f3;
  --t-fg: rgb(0 0 0 / 0.9);
  --t-fg-inactive: rgb(0 0 0 / 0.45);
  --t-hover: rgb(0 0 0 / 0.06);
  --t-pressed: rgb(0 0 0 / 0.04);
}
.cmdr-demo-titlebar[data-look='windows'] .cmdr-demo-titlebar-title { padding-inline: 16px; }
.cmdr-demo-titlebar[data-look='windows'] .cmdr-demo-titlebar-button { width: 46px; height: 100%; }
.cmdr-demo-titlebar[data-look='windows'] .cmdr-demo-titlebar-button:not([aria-disabled='true']):hover { background: var(--t-hover); }
.cmdr-demo-titlebar[data-look='windows'] .cmdr-demo-titlebar-button:not([aria-disabled='true']):active { background: var(--t-pressed); }
.cmdr-demo-titlebar[data-look='windows'] [data-button='close']:not([aria-disabled='true']):hover { background: #c42b1c; color: #fff; }
.cmdr-demo-titlebar[data-look='windows'] [data-button='close']:not([aria-disabled='true']):active { background: rgb(196 43 28 / 0.9); color: #fff; }

/* Linux (desktop-neutral): the title centered in bold, three 24 px round buttons on the right,
   tinted on hover, as GTK's default title bar draws them. */
.cmdr-demo-titlebar[data-look='linux'] {
  height: ${String(TITLE_BAR_HEIGHT.linux)}px;
  font-size: 13px;
  font-weight: 700;
  border-bottom: 1px solid var(--t-border);
  --t-bg: #ebebeb;
  --t-fg: rgb(0 0 0 / 0.8);
  --t-fg-inactive: rgb(0 0 0 / 0.4);
  --t-hover: rgb(0 0 0 / 0.08);
  --t-pressed: rgb(0 0 0 / 0.14);
  --t-border: rgb(0 0 0 / 0.12);
}
/* The buttons' width on the left too, so the title centers on the bar. */
.cmdr-demo-titlebar[data-look='linux']::before { content: ''; width: 96px; flex-shrink: 0; }
.cmdr-demo-titlebar[data-look='linux'] .cmdr-demo-titlebar-title { text-align: center; }
.cmdr-demo-titlebar[data-look='linux'] .cmdr-demo-titlebar-buttons { width: 96px; gap: 6px; justify-content: flex-end; padding-inline-end: 8px; box-sizing: border-box; }
.cmdr-demo-titlebar[data-look='linux'] .cmdr-demo-titlebar-button { width: 24px; height: 24px; border-radius: 50%; }
.cmdr-demo-titlebar[data-look='linux'] .cmdr-demo-titlebar-button:not([aria-disabled='true']):hover { background: var(--t-hover); }
.cmdr-demo-titlebar[data-look='linux'] .cmdr-demo-titlebar-button:not([aria-disabled='true']):active { background: var(--t-pressed); }

@media (prefers-color-scheme: dark) {
  .cmdr-demo-titlebar[data-look='windows'] {
    --t-bg: #202020;
    --t-fg: #fff;
    --t-fg-inactive: rgb(255 255 255 / 0.45);
    --t-hover: rgb(255 255 255 / 0.06);
    --t-pressed: rgb(255 255 255 / 0.04);
  }
  .cmdr-demo-titlebar[data-look='linux'] {
    --t-bg: #303030;
    --t-fg: #fff;
    --t-fg-inactive: rgb(255 255 255 / 0.45);
    --t-hover: rgb(255 255 255 / 0.1);
    --t-pressed: rgb(255 255 255 / 0.16);
    --t-border: rgb(0 0 0 / 0.36);
  }
}
`

/** Injects the bars' stylesheet once per page. */
function ensureStyle(): void {
  if (document.getElementById('cmdr-demo-titlebar-style')) return
  const style = document.createElement('style')
  style.id = 'cmdr-demo-titlebar-style'
  style.textContent = STYLE
  document.head.append(style)
}

const LABEL_KEYS = {
  // The traffic lights' word too: one "Minimize" in every language.
  minimize: 'menu.window.minimize',
  maximize: 'ui.demoWindow.maximize',
  restore: 'ui.demoWindow.restore',
  close: 'ui.modalDialog.close',
} as const

/** A window's title bar in the `look` of the visitor's OS, with `title` and the three buttons. */
export function createTitleBar(look: TitleBarLook, title: string, actions: TitleBarActions): TitleBar {
  ensureStyle()
  const root = document.createElement('div')
  root.className = 'cmdr-demo-titlebar'
  root.dataset.look = look
  root.dataset.active = 'true'
  root.dataset.zoom = 'enterFullScreen'
  const text = document.createElement('div')
  text.className = 'cmdr-demo-titlebar-title'
  text.textContent = title
  const group = document.createElement('div')
  group.className = 'cmdr-demo-titlebar-buttons'
  group.setAttribute('role', 'group')
  const buttons: Partial<Record<'minimize' | 'maximize' | 'close', HTMLButtonElement>> = {}
  for (const kind of ['minimize', 'maximize', 'close'] as const) {
    const button = document.createElement('button')
    button.type = 'button'
    button.className = 'cmdr-demo-titlebar-button'
    button.dataset.button = kind
    // The caption buttons aren't Tab stops on either OS: the keyboard has Alt+F4, Esc, and the menu.
    button.tabIndex = -1
    const action = actions[kind]
    if (action) button.addEventListener('click', action)
    else button.setAttribute('aria-disabled', 'true')
    buttons[kind] = button
    group.append(button)
  }
  root.append(text, group)
  // Each button's name, in the UI language now and after every switch; maximize's by the zoom state.
  const relabel = () => {
    buttons.minimize?.setAttribute('aria-label', getMessage(LABEL_KEYS.minimize))
    buttons.close?.setAttribute('aria-label', getMessage(LABEL_KEYS.close))
    const zoomed = root.dataset.zoom === 'exitFullScreen'
    buttons.maximize?.setAttribute('aria-label', getMessage(zoomed ? LABEL_KEYS.restore : LABEL_KEYS.maximize))
  }
  const stopFollowing = followLanguage(relabel)
  // A press on the bar (not a button) moves the window; a double click maximizes it, as both OSes do.
  root.addEventListener('mousedown', (event) => {
    if (event.button !== 0 || (event.target as Element).closest('button')) {
      if ((event.target as Element).closest('button')) event.preventDefault()
      return
    }
    event.preventDefault()
    actions.drag?.(event)
  })
  root.addEventListener('dblclick', (event) => {
    if (!(event.target as Element).closest('button')) actions.maximize?.()
  })
  return {
    root,
    setActive: (active) => {
      root.dataset.active = String(active)
    },
    setZoomGlyph: (zoom) => {
      root.dataset.zoom = zoom
      relabel()
    },
    destroy: stopFollowing,
  }
}

/**
 * The main window's title bar, first in the page above the menu bar. Maximize is the browser's
 * full screen, the nearest a page gets (inert where the page can't); minimize and close stay
 * inert, since a page can't hide or close its own tab. Sets {@link TITLE_BAR_HEIGHT_PROPERTY} on
 * `:root`, so the demo's chrome that sits on the main window starts under the bar and the menu bar.
 */
export function createMainWindowTitleBar(look: TitleBarLook): TitleBar {
  const toggleFullScreen = () => {
    void setBrowserFullScreen(!isBrowserFullScreen())
  }
  const bar = createTitleBar(look, MAIN_WINDOW_TITLE, canFullScreen() ? { maximize: toggleFullScreen } : {})
  bar.root.classList.add('cmdr-demo-titlebar-main')
  document.documentElement.style.setProperty(TITLE_BAR_HEIGHT_PROPERTY, `${String(TITLE_BAR_HEIGHT[look])}px`)
  document.addEventListener('fullscreenchange', () => {
    bar.setZoomGlyph(document.fullscreenElement ? 'exitFullScreen' : 'enterFullScreen')
  })
  return bar
}
