import { RuleTester } from 'eslint'
import * as svelteParser from 'svelte-eslint-parser'
import tseslint from 'typescript-eslint'
import rule from './no-tracked-focus-in-effect.js'

// Flat-config RuleTester (ESLint 9+). RuleTester auto-detects Vitest's `describe`/`it` globals,
// so `run` is called at the top level. Two testers: a `.svelte.ts` module and a `.svelte` script.
const tsTester = new RuleTester({
  languageOptions: { parser: tseslint.parser, ecmaVersion: 'latest', sourceType: 'module' },
})
const svelteTester = new RuleTester({
  languageOptions: {
    parser: svelteParser,
    parserOptions: { parser: tseslint.parser },
    ecmaVersion: 'latest',
    sourceType: 'module',
  },
})

const MODULE = 'src/lib/file-explorer/views/list-focus.svelte.ts'
const COMPONENT = 'src/lib/file-explorer/pane/DualPaneExplorer.svelte'

/** A `.svelte` component whose script is `body`. */
function component(body) {
  return `<script lang="ts">\n${body}\n</script>\n`
}

tsTester.run('no-tracked-focus-in-effect', rule, {
  valid: [
    // The fixes.
    { code: `$effect(() => { if (initialized) untrack(focusActivePane) })`, filename: MODULE },
    {
      code: `$effect(() => { dependOn(request); const box = textarea; untrack(() => box?.focus()) })`,
      filename: MODULE,
    },
    { code: `$effect(() => { const l = getListbox(); if (l) untrack(() => focusLaidOut(l)) })`, filename: MODULE },
    // Deferred: runs after the effect, untracked.
    { code: `$effect(() => { void tick().then(() => { focusAndSelect() }) })`, filename: MODULE },
    { code: `$effect(() => { setTimeout(() => el.focus(), 0) })`, filename: MODULE },
    { code: `$effect(() => { queueMicrotask(() => { if (isFocusAdrift()) focusHome() }) })`, filename: MODULE },
    { code: `$effect(() => { requestAnimationFrame(() => input.select()) })`, filename: MODULE },
    // A handler, and the cleanup, don't run in the effect body.
    { code: `$effect(() => { el.addEventListener('x', () => el.focus()) })`, filename: MODULE },
    { code: `$effect(() => () => { queueMicrotask(() => focusHome()) })`, filename: MODULE },
    { code: `$effect(() => { return () => el.focus() })`, filename: MODULE },
    { code: `$effect(() => { const later = () => el.focus(); register(later) })`, filename: MODULE },
    { code: `$effect(() => { rowState.setBatchApplier((diff) => { onRequestFocus() }) })`, filename: MODULE },
    { code: `$effect(() => registerFocusHome({ root, refocus: focusActivePane }))`, filename: MODULE },
    // Words that only contain the letters: a predicate, a getter, a read.
    {
      code: `$effect(() => { if (isFocusedPane() && isFocusAdrift() && getSelection() && selectedCount() > 0) mark() })`,
      filename: MODULE,
    },
    // Outside an effect.
    { code: `function submit() { textarea?.focus() }`, filename: MODULE },
    { code: `onMount(() => { el.focus() })`, filename: MODULE },
    // (The per-line opt-out is ESLint's own `eslint-disable-next-line`, which RuleTester can't
    // name with the `cmdr/` prefix, so the real lint run is what proves it.)
  ],
  invalid: [
    // The original #1464 shape: the explorer's startup effect, through a named seam.
    {
      code: `$effect(() => { if (initialized) { focusActivePane() } })`,
      filename: MODULE,
      errors: [{ messageId: 'trackedFocus', data: { name: 'focusActivePane' } }],
    },
    // The composer's shape: a tracked `.focus()` beside the read that should re-run it.
    {
      code: `$effect(() => { dependOn(askCmdrState.composerFocusRequest); textarea?.focus() })`,
      filename: MODULE,
      errors: [{ messageId: 'trackedFocus', data: { name: 'focus' } }],
    },
    {
      code: `$effect.pre(() => { input.select() })`,
      filename: MODULE,
      errors: [{ messageId: 'trackedFocus', data: { name: 'select' } }],
    },
    // A tracked focus beside an untracked read is still tracked.
    {
      code: `$effect(() => { untrack(() => section)?.focus() })`,
      filename: MODULE,
      errors: [{ messageId: 'trackedFocus', data: { name: 'focus' } }],
    },
    // A callback that runs on the spot is the effect's body too.
    {
      code: `$effect(() => { items.forEach((item) => item.focus()) })`,
      filename: MODULE,
      errors: [{ messageId: 'trackedFocus', data: { name: 'focus' } }],
    },
    {
      code: `$effect(() => { if (listbox && isFocusAdrift()) focusLaidOut(listbox) })`,
      filename: MODULE,
      errors: [{ messageId: 'trackedFocus', data: { name: 'focusLaidOut' } }],
    },
    {
      code: `$effect(() => { refocus(); selectAll() })`,
      filename: MODULE,
      errors: [
        { messageId: 'trackedFocus', data: { name: 'refocus' } },
        { messageId: 'trackedFocus', data: { name: 'selectAll' } },
      ],
    },
  ],
})

svelteTester.run('no-tracked-focus-in-effect (Svelte)', rule, {
  valid: [{ code: component(`$effect(() => { if (initialized) untrack(focusActivePane) })`), filename: COMPONENT }],
  invalid: [
    {
      code: component(`$effect(() => {\n  if (initialized) {\n    focusActivePane()\n  }\n})`),
      filename: COMPONENT,
      errors: [{ messageId: 'trackedFocus', data: { name: 'focusActivePane' } }],
    },
    {
      code: component(`$effect(() => {\n  dependOn(askCmdrState.composerFocusRequest)\n  textarea?.focus()\n})`),
      filename: 'src/lib/ask-cmdr/AskCmdrComposer.svelte',
      errors: [{ messageId: 'trackedFocus', data: { name: 'focus' } }],
    },
  ],
})
