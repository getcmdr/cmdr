/**
 * Tests for the uncaught-error forwarder.
 *
 * The point of the module is that a frontend crash becomes a log line, so these
 * assert the two window events reach `log.error`, that a resource-load `error`
 * event does NOT (it isn't a crash), and that neither listener swallows the event.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'

const h = vi.hoisted(() => ({ error: vi.fn(), warn: vi.fn() }))

vi.mock('./logger', () => ({
  getAppLogger: () => ({ debug: vi.fn(), info: vi.fn(), warn: h.warn, error: h.error }),
  whenLoggerReady: (emit: () => void) => {
    emit()
  },
}))

import { logPageCrash, registerUncaughtErrorLogging } from './uncaught-errors'

// The module registers once per process, so register here and let every case
// dispatch into the same listeners — which also proves the idempotence guard
// doesn't stop the FIRST registration from working.
registerUncaughtErrorLogging()

beforeEach(() => {
  h.error.mockClear()
  h.warn.mockClear()
})

describe('uncaught error logging', () => {
  it('logs an uncaught error with its origin and stack', () => {
    const boom = new Error('kaboom')
    boom.stack = 'Error: kaboom\n    at somewhere'
    window.dispatchEvent(
      new ErrorEvent('error', { error: boom, message: 'kaboom', filename: 'app.js', lineno: 12, colno: 3 }),
    )

    expect(h.error).toHaveBeenCalledTimes(1)
    const [, ctx] = h.error.mock.calls[0] as [string, { source: string; detail: unknown }]
    expect(ctx.source).toBe('app.js:12:3')
    expect(ctx.detail).toBe(boom)
  })

  it('keeps the message when the engine leaves it out of the stack', () => {
    // ❗ WebKit's `error.stack` is FRAMES ONLY: no `Name: message` header, unlike
    // V8's. Cmdr ships on WKWebView, so taking the stack verbatim logged every
    // uncaught error as an anonymous pile of minified frames. That cost a real
    // diagnosis: a Svelte flush throw in the former Servers hub logged nothing but
    // offsets.
    const boom = new Error('each_key_duplicate')
    boom.stack = 'flush@app.js:1:17123\n@app.js:1:6396'
    window.dispatchEvent(new ErrorEvent('error', { error: boom, message: '', filename: 'app.js', lineno: 1, colno: 1 }))

    const [, ctx] = h.error.mock.calls[0] as [string, { detail: unknown; stack: string }]
    expect(ctx.detail).toBe(boom)
    expect(ctx.stack).toContain('flush@app.js')
  })

  it('puts the thrown value raw in {detail} and the frames in {stack}, so a report redacts and caps the message', () => {
    // `{detail}` renders as a `detail="…"` field, which the report pass redacts (paths with any
    // prefix included) and caps. It goes in RAW: the bridge renders it through `describeForLog`,
    // which marks identity values, and a pre-rendered string would have its markers scrubbed.
    // The frames are the app bundle's own URLs.
    const event = new Event('unhandledrejection') as Event & { reason: unknown }
    const reason = new Error('Permission denied: /srv/data/Private Person/Medical records')
    reason.stack = 'Error: Permission denied: /srv/data/Private Person/Medical records\n    at load (app.js:1:2)'
    event.reason = reason
    window.dispatchEvent(event)

    const [template, ctx] = h.error.mock.calls[0] as [string, { detail: unknown; stack: string }]
    expect(template).toBe('Unhandled promise rejection: {detail}\n{stack}')
    expect(ctx.detail).toBe(reason)
    expect(ctx.stack).toBe('    at load (app.js:1:2)')
  })

  it('logs an unhandled rejection', () => {
    // jsdom doesn't construct PromiseRejectionEvent, so dispatch the shape the
    // listener reads. A real rejection carries the same two fields.
    const event = new Event('unhandledrejection') as Event & { reason: unknown }
    event.reason = new Error('promise went bad')
    window.dispatchEvent(event)

    expect(h.error).toHaveBeenCalledTimes(1)
    const [, ctx] = h.error.mock.calls[0] as [string, { detail: unknown }]
    expect(ctx.detail).toBe(event.reason)
  })

  it('logs a message-only error event at warn, so a browser notice never auto-sends a report', () => {
    // WebKit fires `error` with a message and NO thrown value for its "ResizeObserver loop
    // completed with undelivered notifications" notice (cmdr-reports#86). Error is the auto-report
    // threshold, and a real throw always carries its value, so the typed check is `event.error`.
    window.dispatchEvent(
      new ErrorEvent('error', { message: 'ResizeObserver loop completed', filename: 'app.js', lineno: 0, colno: 0 }),
    )

    expect(h.error).not.toHaveBeenCalled()
    expect(h.warn).toHaveBeenCalledTimes(1)
    const [template, ctx] = h.warn.mock.calls[0] as [string, { source: string; detail: unknown }]
    expect(template).toBe('Uncaught error event without a thrown value at {source}: {detail}')
    expect(ctx.source).toBe('app.js:0:0')
    expect(ctx.detail).toBe('ResizeObserver loop completed')
  })

  it('ignores a resource-load error, which is not a crash', () => {
    // A failed `<img>` / `<script>` load fires `error` with no `error` and no message.
    window.dispatchEvent(new ErrorEvent('error', { message: '' }))

    expect(h.error).not.toHaveBeenCalled()
  })

  it('does not swallow the event, so browser reporting and hmr-recovery still see it', () => {
    const event = new ErrorEvent('error', { error: new Error('x'), message: 'x', cancelable: true })
    window.dispatchEvent(event)

    expect(h.error).toHaveBeenCalledTimes(1)
    expect(event.defaultPrevented).toBe(false)
  })

  it('logs a page crash SvelteKit caught, and hands back its message for the error page', () => {
    // SvelteKit catches a render throw itself and shows its error page, so the window
    // listeners never see it. `hooks.client.ts` routes it here instead.
    const boom = new TypeError('undefined is not an object')
    boom.stack = 'render@app.js:1:42'

    const detail = logPageCrash({ error: boom, routeId: '/(main)' })

    expect(detail).toBe('TypeError: undefined is not an object')
    const [template, ctx] = h.error.mock.calls[0] as [string, { routeId: string; detail: unknown; stack: string }]
    expect(template).toBe('Page crashed in {routeId}: {detail}\n{stack}')
    expect(ctx.routeId).toBe('/(main)')
    expect(ctx.detail).toBe(boom)
    expect(ctx.stack).toBe('render@app.js:1:42')
  })

  it('hands the error page plain text, never log markers', () => {
    // `describeForLog` marks identity-keyed values for the report redactor; a marked string is
    // log text and must never reach the screen or the report note.
    const thrown = { path: '/Users/someone/Secret plans.txt' }

    const detail = logPageCrash({ error: thrown, routeId: '/(main)' })

    expect(detail).toBe('{"path":"/Users/someone/Secret plans.txt"}')
    const [, ctx] = h.error.mock.calls[0] as [string, { detail: unknown }]
    expect(ctx.detail).toBe(thrown)
  })

  it('registers only once, so an HMR re-import cannot double-log', () => {
    registerUncaughtErrorLogging()
    registerUncaughtErrorLogging()

    window.dispatchEvent(new ErrorEvent('error', { error: new Error('once'), message: 'once' }))

    expect(h.error).toHaveBeenCalledTimes(1)
  })
})
