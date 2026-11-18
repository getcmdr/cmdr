# IPC transport: postMessage fallback vs. the `ipc:` custom protocol (2026-11-18)

Issue #1234. The CSP's `connect-src 'self'` blocked Tauri's `ipc://localhost` fetch, so every window ran on the
`postMessage` fallback. These numbers are from before and after adding `ipc:` to `connect-src`.

## Method

- macOS 27.0.1, Apple silicon laptop, on AC, other work running (a normal dev machine, not idle).
- E2E build (`pnpm test:e2e:playwright:build`, `e2e` profile, optimized), tauri 2.12.5, launched with an isolated
  `CMDR_DATA_DIR`, one main window.
- A throwaway Playwright spec evaluated a script in the main webview: open a listing of a folder with 5,000 files
  (`list_directory_start_streaming`), then time `window.__TAURI_INTERNALS__.invoke('get_file_range', …)` in a loop.
  Warm-up first (30 one-row calls, five full calls). Mean per call over the whole loop (`performance.now()` is clamped
  to 1 ms in WKWebView, so per-call medians aren't resolvable; the mean over the loop is).
- Transport check: a wrapped `window.fetch` counted `ipc` fetches during the timed loops: 0 on the old CSP (fallback),
  1,341–1,584 on the new one (custom protocol).
- Payload sizes (JSON of the rows): one row ~0.7 KB, a 200-row page 136 KB, all 5,000 rows 3.4 MB.
- Runs interleaved: old, new, new, old (two old-CSP runs also include Playwright's retry of the first, same build).

## Results (mean ms per call)

- One row (1,000 calls): old 0.147, 0.150, 0.151; new 0.271, 0.347. The custom protocol costs ~0.15 ms MORE per tiny
  call.
- A 200-row page, what a pane fetches while scrolling (300 calls): old 0.613, 0.607, 0.663; new 0.547, 0.527. ~15 %
  faster.
- 5,000 rows, 3.4 MB (40 calls): old 11.9, 11.8, 11.8; new 6.8, 7.0. ~1.7x faster.

## Reading

- The fetch path wins once a payload passes roughly 100 KB, and loses ~0.15 ms on a call that carries almost nothing. At
  Cmdr's call rates (a handful of IPC calls per keypress) the small-call cost stays well under a frame; the big answers
  (a full listing, the viewer's byte reads, which skip JSON entirely on this path) are where time was going.
- Not measured: binary answers (`Vec<u8>` / `ArrayBuffer`), which the fallback has to JSON-encode as number arrays, so
  the gap there is larger than the 3.4 MB JSON case; and Windows (WebView2) and Linux (WebKitGTK).

## Why it was reverted (2026-11-18)

The switch changes ORDERING, not just speed. Events always reach the webview as a `webview.eval` (tauri 2.12.5
`event/listener.rs` `emit_js_filter`). On the `postMessage` path a command's answer is an `eval` too (`ipc/protocol.rs`
`responder_eval`), queued in order with the events; on the custom protocol it is the body of a `fetch` response, which
WebKit resolves on its own schedule. So an operation's events can now run in the webview before its start command's
answer does.

That exposed a frontend bug: the corner chip bound a session to the not-yet-claimed operation, drained its
`write-complete`, and let go as the progress dialog claimed it, so the dialog closed a finished operation as cancelled
(`apps/desktop/src/lib/file-operations/operation-session/DETAILS.md` § "How an operation ended"; both sides fixed).
Linux E2E (Docker, WebKitGTK, ubuntu 26.04 arm64), the copy F5 / move F6 / MTP delete trio at `--repeat-each 20`, MTP
deletes whose first attempt failed: `ipc:` on, 16 and 17 of 20 (two runs); `ipc:` off, 0 of 20; `ipc:` on plus the
frontend fix, 0 of 20 (×3). macOS 27.0.1 E2E build, `ipc:` on, unfixed: 3 of 20. Copies and moves lost nothing locally
(0 of 40 per run), though CI lost one of each on commit 475a87c074.

Before re-enabling it: measure on WebKitGTK and WebView2 too, and look for any other place that assumes a command's
answer lands before the events it causes.
