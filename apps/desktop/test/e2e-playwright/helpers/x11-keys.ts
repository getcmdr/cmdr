/**
 * Real X key and mouse events for the Linux lane, through the packaged `xdotool` (XTest, so the X
 * server and GTK see a device keypress). `pressKey` builds a synthetic DOM event inside the
 * webview, which GTK never sees: anything GTK could take before the page (F10, a lone Alt, the
 * F-keys) needs this, as does anything that must prove GTK DIDN'T take it. Linux only; `xdotool`
 * comes with the lane's base image.
 *
 * The Xvfb display has no window manager, so nothing gives a window focus by itself:
 * `focusMainWindow` sets the X input focus and parks the pointer on the biggest visible
 * top-level window (the main window), which is where XTest key events land.
 */
import { execFileSync } from 'node:child_process'

function xdotool(...args: string[]): string {
  return execFileSync('xdotool', args, { encoding: 'utf8' })
}

/** The visible direct children of the root window, by X id. An open GTK menu is one of these. */
export function visibleTopLevelWindows(): string[] {
  try {
    // `--name ''` matches every window; xdotool exits 1 when nothing matches.
    return xdotool('search', '--onlyvisible', '--maxdepth', '1', '--name', '')
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line !== '')
  } catch {
    return []
  }
}

function size(windowId: string): { width: number; height: number } {
  const shell = xdotool('getwindowgeometry', '--shell', windowId)
  return {
    width: Number(/WIDTH=(\d+)/.exec(shell)?.[1] ?? 0),
    height: Number(/HEIGHT=(\d+)/.exec(shell)?.[1] ?? 0),
  }
}

function area(windowId: string): number {
  const { width, height } = size(windowId)
  return width * height
}

/** The main window's X id: the biggest visible top-level window. */
export function mainWindowId(): string {
  const windows = visibleTopLevelWindows()
  if (windows.length === 0) throw new Error('no visible X window to focus')
  return windows.reduce((best, id) => (area(id) > area(best) ? id : best))
}

/** Gives the main window the X input focus and the pointer, so real keys reach it. */
export function focusMainWindow(): void {
  const main = mainWindowId()
  xdotool('windowfocus', '--sync', main)
  xdotool('mousemove', '--window', main, '400', '300')
}

/**
 * Clicks one X window at a point in its own coordinates, as a real mouse does: the window gets the
 * input focus, the pointer moves there, and button 1 goes down and up. A page point (a DOM rect's)
 * is the window's point here: the lane's windows have no decorations and the page fills them.
 */
export function clickRealAt(windowId: string, x: number, y: number): void {
  xdotool('windowfocus', '--sync', windowId)
  xdotool('mousemove', '--window', windowId, String(Math.round(x)), String(Math.round(y)))
  xdotool('click', '1')
}

/**
 * Gives one X window (an id from `visibleTopLevelWindows`) the input focus and clicks its middle,
 * so GTK's own focus lands on the webview: a window that opened under a bar starts with no focused
 * widget, and real keys go nowhere until the page is clicked, as a user would.
 */
export function focusWindow(windowId: string): void {
  const { width, height } = size(windowId)
  xdotool('windowfocus', '--sync', windowId)
  xdotool('mousemove', '--window', windowId, String(Math.floor(width / 2)), String(Math.floor(height / 2)))
  xdotool('click', '1')
}

/**
 * Gives one X window the input focus and clicks near its bottom-left corner, so GTK's own focus
 * lands on the webview without the click hitting a control in the middle of the page.
 */
export function focusWindowAtCorner(windowId: string): void {
  const { height } = size(windowId)
  xdotool('windowfocus', '--sync', windowId)
  xdotool('mousemove', '--window', windowId, '8', String(Math.max(0, height - 8)))
  xdotool('click', '1')
}

/**
 * The F-keys' X keycodes (evdev's numbering), which `pressRealKey` sends in place of their names:
 * by its keysym, the image's xdotool (3.20160805) adds Alt to every F-key, since the keymap types
 * them `CTRL+ALT` (`xev` in the lane's image, 2026-11-19), so a bare `F8` arrived as Alt+F8. A
 * keycode goes through as is, and a modifier beside it stays its own (`shift+74` is Shift+F8).
 * Regression anchor (review of #1281): the step 6 and 7 reviews' "F8 in Settings touched nothing"
 * had pressed Alt+F8.
 */
const F_KEY_CODES: Readonly<Record<string, string>> = {
  F1: '67',
  F2: '68',
  F3: '69',
  F4: '70',
  F5: '71',
  F6: '72',
  F7: '73',
  F8: '74',
  F9: '75',
  F10: '76',
  F11: '95',
  F12: '96',
}

/** `chord` with each F-key named by its keycode (`F_KEY_CODES`), so it arrives bare. Exported for its test. */
export function xdotoolChord(chord: string): string {
  return chord
    .split('+')
    .map((part) => F_KEY_CODES[part] ?? part)
    .join('+')
}

/**
 * Presses one chord as a real keyboard would, in xdotool's spelling (`alt+f`, `Escape`, `F8`,
 * `shift+F8`). An F-key arrives bare, with only the modifiers the chord names (`xdotoolChord`).
 */
export function pressRealKey(chord: string): void {
  xdotool('key', '--clearmodifiers', xdotoolChord(chord))
}
