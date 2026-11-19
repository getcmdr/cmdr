# Browser demo (`lib/demo/`)

The real Cmdr frontend in a plain browser on a fake backend, for getcmdr.com/demo (issue #302). Every `invoke` lands in
a typed answer here; listing commands go to the real listing engine, built to WASM
(`crates/cmdr-listing-wasm/CLAUDE.md`).

- `boot.ts`: the frame's role. `backend.ts`: the page's one backend. `webview.ts`: one frame's `__TAURI_INTERNALS__`.
  `windows.ts`: secondary windows as in-page panels (Settings, Keyboard shortcuts, the queue, `viewer-*`).
  `menu-bar.svelte.ts` and `context-menu.ts`: the menu bar and right-click menus, from `menu-engine.ts` (the desktop's
  bar through WASM), on `#lib/ui`'s menus, clicks through `menu-click.ts`. `traffic-lights.ts` (Mac) / `title-bar.ts`
  (Windows, Linux): the window buttons. `drag-regions.ts`: Tauri's `drag.js`. `drag-session.ts` / `os-drop.ts`: drags
  out and drops in. `color-scheme.ts`: `setTheme` per frame.
- `answers.ts`, `volumes.ts`, `favorites.ts` (in memory), `relayed.ts` (answers that emit), `plugins.ts` (in-memory
  `plugin:store`, path, app), `browser-dialogs.ts` (questions, pickers), `opener.ts` (links open in a new tab),
  `listing.ts` (routes to the WASM engine, loaded by `engine-module.ts`), `coverage.ts` (every command answered or left
  out with a reason; `ipc-calls.ts` reads the raw ones off the app's sources), `own-window.ts` (what a webview answers
  itself), `unanswered.ts` (a void resolves, any other rejects; an undecided gap logs an error into
  `window.__cmdrDemo.unanswered`).
- `demo-tree.ts`, `demo-contents.ts`, and `browser-shortcuts.ts`: PLACEHOLDER content David still decides.

Run: `pnpm dev:demo` or `pnpm build:demo` (into `apps/desktop/build-demo/`, base `/demo`) in `apps/desktop`. Both build
the WASM engine first.

## Must-knows

- **Isolation is a hard rule.** The app-side footprint is the `hooks.client.ts` import, the `vite.config.js` define,
  aliases, and demo-only void-commands plugin, and the `app.d.ts` global. No `if (demo)` in app code: demo behavior goes
  through existing seams (an IPC answer, a store seed, a typed refusal). `cmdr/demo-isolation` fails any
  `__CMDR_DEMO_BUILD__` or `#lib/demo` outside.
- **No demo code in any other build.** Outside `CMDR_DEMO_BUILD=1`, `vite.config.js` aliases `#lib/demo/boot.js` to the
  empty `boot-absent.ts`. Verify: a normal `pnpm build` has no `__cmdrDemo`.
- **One backend per page, in the top frame; one webview per frame.** A secondary window is a same-origin iframe of its
  route, named after its window label, attached through `window.parent.__cmdrDemoBackend`. Events fan out to every
  attached webview. Keep per-window facts (size, focus) in `webview.ts`, app state in the backend.
- **The demo may itself be framed** (a landing-page embed): never touch `window.parent` / `window.top` beyond the
  guarded backend lookup.
- **Answers are typed against `bindings.ts`** (`AnswerTable`, `VoidCommand`), so a drifted answer fails `svelte-check`.
  An error branch is `refuse(command, typedError)`, never a thrown `Error`.
- **A new backend call needs an answer or a reason it's left out** (`coverage.ts`): a bindings command fails
  `svelte-check` and `coverage.test.ts`; a raw `invoke` or a Tauri JS API import fails `ipc-calls.test.ts`. Only a
  window or webview handle's methods are left to the runtime net (`unanswered`). An engine command goes in
  `engine-command-names.ts` too (a Rust test pins it to the crate).
- **No menu logic in TS.** Rows, words, shortcuts, enabled and checked state, and what a click sends all come from
  `cmdr-menu` through `DemoMenuBar`, the right-click menus' too. A click sends the native item's own event, so the app's
  handlers are the ones that run.
- **No listing or write logic in TS.** The backend tries its TS tables before the engines, so a TS answer for an engine
  command silently shadows it. `engine-commands.test.ts` pins every engine command as engine-only.
- **Writes go through the real write engine, in memory, gone on reload** (`operations_host/`). No Trash on the volume,
  so F8 takes the app's permanent-delete confirm. ❌ Never a TS success for a write.
- **No top-level `await`** in the boot path: WebKit can run an importer early, and Safari visitors run the demo.

Adding an answer, the frame model, the seam, the placeholders, and decisions: `DETAILS.md`.
