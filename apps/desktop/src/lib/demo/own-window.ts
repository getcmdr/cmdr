/**
 * What a webview answers itself (`webview.ts`): its own window's facts, which the frame's viewport
 * stands in for, a question over its window, and the event plugin's commands, whose listeners
 * live in that frame.
 */
import type { Payload } from './answer-types.js'
import { isBrowserFullScreen, setBrowserFullScreen } from './browser-full-screen.js'
import { askQuestion } from './question-dialog.js'

const physical = () => ({
  width: Math.round(window.innerWidth * window.devicePixelRatio),
  height: Math.round(window.innerHeight * window.devicePixelRatio),
})

/** Commands about this webview's own window: the frame's viewport stands in for it. */
const windowAnswers: Partial<Record<string, (payload: Payload) => unknown>> = {
  'plugin:window|outer_size': physical,
  'plugin:window|inner_size': physical,
  'plugin:window|scale_factor': () => window.devicePixelRatio,
  // The browser's full screen stands in for the window's, so Toggle full screen and Escape's
  // exit drive it (where the page may; a refusal answers like a done call).
  'plugin:window|is_fullscreen': isBrowserFullScreen,
  'plugin:window|set_fullscreen': async (payload) => {
    await setBrowserFullScreen(payload.value === true)
    return null
  },
  'plugin:window|is_maximized': () => false,
  'plugin:window|is_focused': () => document.hasFocus(),
  'plugin:window|is_visible': () => true,
  'plugin:window|title': () => document.title,
  // A frame sits at its own origin; the panel around a secondary window is the page's business.
  'plugin:window|outer_position': () => ({ x: 0, y: 0 }),
  'plugin:window|inner_position': () => ({ x: 0, y: 0 }),
  // A window's frame is the page's to size and name, not the app's.
  'plugin:window|set_title': () => null,
  'plugin:window|set_min_size': () => null,
  'plugin:window|set_max_size': () => null,
  // A question sits over the window that asked, as the native alert does (`question-dialog.ts`).
  'plugin:dialog|message': askQuestion,
}

/** The event plugin's commands, which each webview keeps (`webview.ts`'s `invoke`). */
export const EVENT_COMMANDS = [
  'plugin:event|listen',
  'plugin:event|unlisten',
  'plugin:event|emit',
  'plugin:event|emit_to',
] as const

/** Every command a webview answers itself, for the demo's coverage (`coverage.ts`). */
export const WEBVIEW_COMMANDS: readonly string[] = [...EVENT_COMMANDS, ...Object.keys(windowAnswers)]

/** The answer to `cmd` about this frame's own window, or `undefined` for a command it doesn't own. */
export function windowAnswer(cmd: string, payload: Payload): (() => unknown) | undefined {
  const answer = windowAnswers[cmd]
  return answer && (() => answer(payload))
}
