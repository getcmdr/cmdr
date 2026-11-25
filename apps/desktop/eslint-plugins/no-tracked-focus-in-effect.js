/**
 * ESLint rule: ban a focus or select call that runs tracked inside a `$effect`.
 *
 * Rationale (#1464): `el.focus()` runs every focus handler on the page synchronously, inside the
 * effect that called it, so each `$state` those handlers read becomes a dependency of that
 * effect. The in-window menu bar's `focusin` reads its `active` state, so an effect that focused
 * the explorer's list (through `focusActivePane`) or the Ask Cmdr composer re-ran every time the
 * bar took the keyboard, pulling focus out of the text field the bar was opened from: the inline
 * rename ended, and Edit > Cut or File > Delete then acted on the file list. Wrap the call in
 * `untrack(() => ...)` (keep the reads that SHOULD re-run the effect outside it).
 *
 * What this rule flags, in a `$effect(...)` / `$effect.pre(...)` body that runs synchronously:
 *   el.focus()                 // `.focus(` and `.select(` on anything
 *   input.select()
 *   focusActivePane()          // any call whose name has `focus` or `select` as a camelCase word:
 *   listFocus.focusList()      // focusX, xFocus, refocus, selectAll, ... (the original bug went
 *   focusLaidOut(listbox)      // through `focusActivePane`, so the name is the only handle)
 *
 * What it leaves alone:
 *   untrack(() => el.focus())                       // the fix
 *   untrack(focusActivePane)                        // same, by reference
 *   void tick().then(() => el.focus())              // deferred: runs after the effect
 *   setTimeout(() => el.focus()), queueMicrotask(...), requestAnimationFrame(...)
 *   el.addEventListener('x', () => el.focus())      // a handler, not the effect body
 *   rowState.setBatchApplier((d) => focusIt())      // `set…`, `on…`, `register…`, `subscribe…`,
 *                                                   // `listen…` store the callback for later
 *   registerFocusHome({ refocus })                  // `register…`, `is…`, `get…` don't move focus
 *   return () => el.focus()                         // the cleanup runs untracked
 *   isFocusAdrift(), getSelection(), selectedCount() // a reader, or `Selection` / `selected`, isn't the verb
 *
 * Opt out per-line, with a reason, when tracking the focus handlers' reads is truly harmless:
 *
 *   // eslint-disable-next-line cmdr/no-tracked-focus-in-effect -- <reason>
 *   el.focus()
 *
 * See `src/lib/file-explorer/pane/DETAILS.md` § Focus model.
 */

/** `focus` or `select` as a whole camelCase word: `focusList`, `refocus`, `selectAll`, not `isFocused`. */
const FOCUS_WORD = /(?:^|(?<=[a-z]))(?:[fF]ocus|[sS]elect)(?=[A-Z0-9_]|$)/
/**
 * A predicate or a getter reads focus, and a registration stores something: neither moves it
 * (`isFocusAdrift`, `getFocusTarget`, `registerFocusHome`).
 */
const READER = /^(?:is|has|get|should|can|was|register|unregister)[A-Z]/
/** A call that STORES its callback for later (`setBatchApplier(fn)`, `onChange(fn)`): the callback isn't the effect's body. */
const STORES_CALLBACK = /^(?:set|register|subscribe|listen|on)[A-Z]/

function movesFocus(name) {
  return FOCUS_WORD.test(name) && !READER.test(name)
}

/** Calls whose callback runs later, outside the effect's tracking. */
const DEFERRING_CALLEES = new Set(['setTimeout', 'queueMicrotask', 'requestAnimationFrame', 'requestIdleCallback'])
/** Methods whose callback runs later, or on an event: `.then(cb)`, `.addEventListener('x', cb)`. */
const DEFERRING_METHODS = new Set(['then', 'catch', 'finally', 'addEventListener'])

function calleeName(callee) {
  if (callee.type === 'Identifier') return callee.name
  if (callee.type === 'MemberExpression' && !callee.computed && callee.property.type === 'Identifier') {
    return callee.property.name
  }
  return null
}

/** `$effect(fn)` or `$effect.pre(fn)`. */
function isEffectCall(node) {
  const callee = node.callee
  if (callee.type === 'Identifier') return callee.name === '$effect'
  return (
    callee.type === 'MemberExpression' &&
    !callee.computed &&
    callee.object.type === 'Identifier' &&
    callee.object.name === '$effect' &&
    callee.property.type === 'Identifier' &&
    callee.property.name === 'pre'
  )
}

/** Whether a call's callback arguments run outside the effect's tracking. */
function defersOrUntracks(call) {
  const name = calleeName(call.callee)
  if (name === null) return false
  if (call.callee.type === 'Identifier' && name === 'untrack') return true
  if (call.callee.type !== 'Identifier' && DEFERRING_METHODS.has(name)) return true
  return DEFERRING_CALLEES.has(name) || STORES_CALLBACK.test(name)
}

function isFunction(node) {
  return node && (node.type === 'ArrowFunctionExpression' || node.type === 'FunctionExpression')
}

/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'problem',
    docs: {
      description:
        'A focus or select call inside a `$effect` must run `untrack`ed, or the focus handlers it runs hand the effect their `$state` reads.',
      recommended: true,
    },
    messages: {
      trackedFocus:
        '`{{ name }}(...)` runs inside this `$effect` with tracking on, so every `$state` a focus handler reads ' +
        "(the menu bar reads its `active`) becomes this effect's dependency and re-runs it later, pulling focus " +
        'from wherever the person is typing (#1464). Wrap it: `untrack(() => {{ name }}(...))`, keeping the reads ' +
        'that should re-run the effect outside. Opt out per-line with ' +
        '`// eslint-disable-next-line cmdr/no-tracked-focus-in-effect -- <reason>`.',
    },
    schema: [],
  },
  create(context) {
    /** Walks `node` as code the effect runs synchronously, reporting tracked focus calls. */
    function walk(node, parent) {
      if (!node || typeof node.type !== 'string') return
      switch (node.type) {
        case 'FunctionDeclaration':
          // Only runs if called; a call to it by a focus-word name is caught at the call.
          return
        case 'ArrowFunctionExpression':
        case 'FunctionExpression':
          // A function value runs synchronously here only when called on the spot (an IIFE) or
          // handed straight to a call (`items.forEach(i => i.focus())`); one stored or returned
          // (the cleanup) doesn't.
          if (parent?.type !== 'CallExpression') return
          walk(node.body, node)
          return
        case 'CallExpression': {
          const name = calleeName(node.callee)
          if (name !== null && movesFocus(name)) {
            context.report({ node, messageId: 'trackedFocus', data: { name } })
          }
          walk(node.callee, node)
          if (defersOrUntracks(node)) {
            // `untrack(focusActivePane)` and `setTimeout(cb)` are fine; their non-function
            // arguments (a delay) can't call anything.
            return
          }
          for (const argument of node.arguments) walk(argument, node)
          return
        }
        default:
          for (const key of Object.keys(node)) {
            if (key === 'parent' || key === 'loc' || key === 'range') continue
            const value = node[key]
            if (Array.isArray(value)) for (const child of value) walk(child, node)
            else if (value && typeof value.type === 'string') walk(value, node)
          }
      }
    }

    return {
      CallExpression(node) {
        if (!isEffectCall(node)) return
        const body = node.arguments[0]
        if (!isFunction(body)) return
        walk(body.body, body)
      },
    }
  },
}
