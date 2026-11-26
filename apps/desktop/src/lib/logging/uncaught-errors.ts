/**
 * Forwards uncaught frontend errors into the app log, so a crash in the webview
 * leaves a trace instead of dying in a console nobody reads.
 *
 * Without this, an uncaught throw or an unhandled promise rejection is invisible
 * everywhere that matters: no line in the Rust log, nothing in an error-report
 * bundle, nothing in a CI E2E run's output. A two-hour CI wedge where the panes
 * stopped listing while the rest of the app stayed healthy produced zero frontend
 * evidence for exactly this reason.
 *
 * Registered once from `routes/+layout.ts` (a stable module, like
 * `hmr-recovery.ts`) so the listeners survive layout re-evaluation during HMR.
 * `log.error` is forwarded to Rust in production too — the prod sinks carry
 * `error+` — so these land in the log file and in error-report bundles. An `error`
 * event with a message but no thrown value logs at warn: it's a browser notice, and
 * error level would auto-send a report for it.
 */

import { describeForLog } from './describe-for-log'
import { getAppLogger, whenLoggerReady } from './logger'

const log = getAppLogger('uncaught')

/**
 * The frames of a thrown value, without the `Name: message` header V8 opens them with. The
 * thrown value itself goes to `{detail}` RAW, which the log bridge renders through
 * `describeForLog` (`Name: message` plus any cause chain, identity values marked) as a
 * `detail="…"` field: an error report redacts and caps it, because an error message can quote a
 * path or a name. Pre-rendering it here would scrub those markers. The frames are the app
 * bundle's own URLs.
 *
 * ❗ **The message is always there, whatever the engine put in the stack.** WebKit's
 * `error.stack` is FRAMES ONLY, where V8's opens with `Name: message`. Cmdr ships on
 * WKWebView, so taking the stack verbatim logged every uncaught error as an anonymous pile of
 * minified offsets — which is exactly what a Svelte flush throw in the former Servers hub looked like,
 * and it cost a real diagnosis. (verified on macOS 26.6.2 / WKWebView and Linux WebKitGTK,
 * reading E2E logs, 2026-09-07)
 */
function framesOf(value: unknown): string {
  if (!(value instanceof Error)) return ''
  const headline = `${value.name}: ${value.message}`
  const stack = value.stack ?? ''
  return stack.startsWith(headline) ? stack.slice(headline.length).replace(/^\n/, '') : stack
}

let registered = false

/**
 * Installs the `error` / `unhandledrejection` listeners. Idempotent, so an HMR
 * re-import can't stack duplicate handlers that log every failure twice.
 *
 * Neither listener calls `preventDefault`: the goal is to OBSERVE, never to
 * swallow. Swallowing would hide the failure from the browser's own reporting and
 * from `hmr-recovery`, which needs to see the rejection it recovers from.
 */
export function registerUncaughtErrorLogging(): void {
  if (registered || typeof window === 'undefined') return
  registered = true

  window.addEventListener('error', (event: ErrorEvent) => {
    // A failed resource load (`<img>`, `<script>`) also fires `error`, carrying
    // neither a thrown value nor a message. Those aren't crashes. The absent value
    // is `null` in some engines and `undefined` in others, so test for both.
    const thrown: unknown = event.error ?? null
    if (thrown === null && !event.message) return
    const { filename, lineno, colno } = event
    const source = filename ? `${filename}:${String(lineno)}:${String(colno)}` : 'unknown'
    if (thrown === null) {
      // A message with no thrown value is a browser notice, not a throw: WebKit's
      // "ResizeObserver loop completed with undelivered notifications" is the one we see
      // (cmdr-reports#86), and it only means a layout change waited a frame. A real throw
      // always carries its value. Warn keeps the line in the log and in any bundle, but
      // stays under the error level that auto-sends a report.
      const message = event.message
      whenLoggerReady(() => {
        log.warn('Uncaught error event without a thrown value at {source}: {detail}', { source, detail: message })
      })
      return
    }
    const detail = thrown
    const stack = framesOf(thrown)
    whenLoggerReady(() => {
      if (stack) log.error('Uncaught error at {source}: {detail}\n{stack}', { source, detail, stack })
      else log.error('Uncaught error at {source}: {detail}', { source, detail })
    })
  })

  window.addEventListener('unhandledrejection', (event: PromiseRejectionEvent) => {
    const detail: unknown = event.reason
    const stack = framesOf(detail)
    whenLoggerReady(() => {
      if (stack) log.error('Unhandled promise rejection: {detail}\n{stack}', { detail, stack })
      else log.error('Unhandled promise rejection: {detail}', { detail })
    })
  })
}

/** What SvelteKit's `handleError` hands over about a throw it caught. */
export interface CaughtPageError {
  error: unknown
  /** The route that was rendering, or `null` when none matched. */
  routeId: string | null
}

/**
 * Logs a throw SvelteKit caught (a component that threw while rendering, or a `load`), and
 * returns it as plain text (`Name: message` plus any cause chain, no log markers) for the error
 * page to show.
 *
 * SvelteKit catches these itself and swaps the window for its error page, so the `window`
 * listeners above never see one: without this the person got a bare "500" and the log got
 * nothing. Called from `hooks.client.ts`.
 */
export function logPageCrash({ error, routeId }: CaughtPageError): string {
  const stack = framesOf(error)
  const route = routeId ?? 'no route'
  whenLoggerReady(() => {
    if (stack) log.error('Page crashed in {routeId}: {detail}\n{stack}', { routeId: route, detail: error, stack })
    else log.error('Page crashed in {routeId}: {detail}', { routeId: route, detail: error })
  })
  return describeForLog(error, { marks: false })
}
