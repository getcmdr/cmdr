# Clipboard details

Depth and rationale. `CLAUDE.md` holds the must-knows; this is the decision record.

## Copy-at-source, decide-at-paste

Follows Finder's model. Cmd+X sets an internal cut flag; the move-vs-copy decision happens at paste time. On macOS the
cut state is Cmdr-internal only, so pasting Cmdr-cut files in Finder does a copy (Finder doesn't know about our flag).
This matches third-party file managers (Path Finder, ForkLift). On Windows the clipboard itself carries the cut
(below), so Explorer moves a Cmdr cut and Cmdr moves an Explorer cut.

## Windows

What goes on the clipboard (`platform/windows.rs`, the bytes in `platform/shell_formats.rs`):

- `CF_HDROP`: a `DROPFILES` header (`pFiles` = 20, `fWide` = 1) then each plain path, NUL-terminated, then a closing
  NUL.
- The registered `Preferred DropEffect`: `DROPEFFECT_MOVE` (2) on a cut, `DROPEFFECT_COPY` (1) on a copy. A read counts
  the files as cut when the move bit is set and the copy bit isn't: Explorer's Ctrl+X writes 2, its Ctrl+C writes 5
  (copy and link).
- `CF_UNICODETEXT`: the paths, one per line (CRLF), as macOS writes `public.utf8-plain-text`.

**Decision: the plain clipboard API (`OpenClipboard` / `SetClipboardData`), not `OleSetClipboard` with an
`IDataObject`.** Why:

- Explorer reads a cut from `Preferred DropEffect` whichever API wrote it: the paste target reads the formats, and the
  plain API renders them up front, so nothing has to stay alive to answer later.
- `OleSetClipboard` needs an STA thread that pumps messages for as long as Cmdr owns the clipboard, plus a hand-written
  `IDataObject`. What it buys is hearing `Paste Succeeded` back, which only matters to a source that ghosts its cut
  items until the paste lands. Cmdr doesn't ghost them (the frontend says "N items ready to move" instead).
- The cost: a delete-on-paste target that COPIES instead of moving (a zip folder, Explorer's phone or FTP namespaces,
  Outlook) tells the source through `SetData`, and Cmdr has no data object to hear it, so a Cmdr cut pasted there ends
  as a copy and the originals stay.
- No owner window: everything is rendered immediately, so nothing ever sends `WM_RENDERFORMAT`. This is what
  `clipboard-win` (under `arboard`) does too.
- Evidence: Microsoft's delete-on-paste procedure (learn.microsoft.com/windows/win32/shell/datascenarios § "Handling
  Delete-on-Paste Operations", read 2026-11-24) names the formats and who does what, and the round trip of these
  formats through the real clipboard is pinned by `platform/windows.rs`'s test on the Windows CI lane. Explorer itself
  honoring a plain-API cut is unverified on a real machine as of 2026-11-24 (issue #942, task 42a's checklist).

**The handshake after Cmdr pastes an Explorer cut** (`cut_paste.rs`): the read notes a marked cut, the move that
starts on exactly those paths claims it (`move_files`, `move_between_volumes`), and `terminal_effects` settles it. A
completion with nothing skipped (`files_skipped == 0`, no Locked item kept) runs `settle_cut` on its own thread:

1. Confirm the clipboard still holds that very cut (a person who copied something since keeps their clipboard).
2. `OleGetClipboard`, then the sequence number again (a new cut that took the clipboard since step 1 mustn't hear a
   paste it didn't get), then `SetData(Performed DropEffect = DROPEFFECT_NONE)` and `SetData(Paste Succeeded =
   DROPEFFECT_MOVE)`, so Explorer clears its ghosted icons. Cmdr's move already took the files (an "optimized move"):
   `NONE` first, as Microsoft's optimized-move procedure does, so a source holding an earlier `Performed DropEffect` of
   move can't read `Paste Succeeded` as "delete now"; ❌ never `Performed DropEffect = DROPEFFECT_MOVE`. A source that
   wrote with the plain API has no data object to take it; that refusal is logged at debug and changes nothing.
3. If the clipboard is unchanged since step 1 (sequence number) and still holds the cut, empty it: a cut pastes once,
   as in Explorer, and a second Ctrl+V doesn't try to move files that are gone.

A cancel, a failure, or a completion that skipped something forgets the claim. A one-file rename can finish before its
start returns, so the ledger remembers the last 64 unclaimed endings, as `drop_in/folders.rs` does.

**A refused cut disarms the earlier one** (`commands/clipboard.rs::disarm_earlier_cut`): a Ctrl+X that ends `Busy` or
`NoPlainPath` leaves the clipboard as it was, so without this the next Ctrl+V would move the files cut BEFORE it. Cmdr's
cut state goes, and on Windows the clipboard is emptied when it still holds exactly that earlier Cmdr cut. An
Explorer cut is never touched (it isn't in Cmdr's cut state).

**Paths coming in are held to the same plain-form rule** (`platform/windows.rs::app_text`, the pure half
`platform/shell_formats.rs::names_its_file`): another app's
plain `C:\x\a.` is `C:\x\a` to Win32 and `C:\x\CON` the device, but the app path would keep the dot and the verbatim
crossing would move a different file. A plain path whose plain form fails `to_native_windows_plain`, a device path, a
drive-relative `C:foo`, a relative path, or a name needing an escape refuses the WHOLE paste as `UnreadablePath`,
naming it (a subset would paste under a toast saying the paste happened). A verbatim `\\?\` path names exactly its file
and passes. `CF_HDROP` blocks whose `pFiles` falls outside the block read as no files, and empty entries are skipped.

**Why plain paths**: `CF_HDROP` readers (Explorer, `DragQueryFileW`, most apps) take the classic form. A verbatim
`\\?\` path would reach some and break others, and stripping the prefix by hand can name another file (Win32 strips a
trailing dot, opens `CON` as a device). So the command crosses each row with `to_native_plain` and refuses the set as
`NoPlainPath`, naming the path, when one has no plain form. Long-path support in Explorer's paste is a later
improvement if it proves reliable.

**OLE vs COM apartments**: `OleGetClipboard` needs `OleInitialize` (an STA with OLE's extras), so `settle_cut` enters
and leaves one on its dedicated thread. The Shell helpers elsewhere (`platform/windows/apartment.rs`) use
`CoInitializeEx`; OLE's clipboard functions are documented to need `OleInitialize` itself.

## Direct NSPasteboard via `objc2`, not a Tauri plugin

The codebase already uses `objc2` for drag image detection, and the official `tauri-plugin-clipboard-manager` only
supports text/images, not file URLs. Direct access gives full control without adding a dependency.

## Cut state in Rust, not the frontend

The backend is authoritative for file operations, so keeping cut state in Rust avoids frontend/backend sync issues. The
frontend queries via IPC when needed. On paste, the backend validates that the live clipboard paths still match the
stored cut-state paths; a mismatch (another app replaced the clipboard) clears the stale cut state and falls back to a
copy.

## E2E mock as a `#[cfg]` module swap, not a `dyn` trait

Three call sites already hop to the main thread via `app.run_on_main_thread()` and pass `PathBuf` values; a trait object
would add `Send` bounds the `objc2` types resist. A `cfg`-driven module swap keeps every call site byte-identical
between configurations and removes the prod-only `objc2` link cost from E2E builds. Acceptance: a full E2E run leaves
`pbpaste` unchanged.

## Runtime `CMDR_CLIPBOARD_BACKEND=mock` override

Lives inside `pasteboard.rs` (not `mod.rs`) because it's a debugging tool for prod-feature builds. Sampled once via
`LazyLock` at first access, so the hot path is a single atomic load. Both the compile-time mock path and this runtime
override share `store.rs`, so a test that flips the env in one process sees the same data the E2E mock module sees in
another. See the "Mock-backend convention" in `docs/tooling/instance-isolation.md`.

## Paste clipboard content as a file (issue #35)

When Cmd+V lands in a pane with no file URLs on the clipboard but some other pasteable content (text, image, PDF), the
backend writes that content to a new `pasted.<ext>` file. The pure core lives in `payload.rs` so precedence, the
markdown sniff, and the flavor mapping are unit-testable with no Tauri runtime or `MainThreadMarker`:

- **`ClipboardData`** (in `store.rs`) is the read bundle: `Option<Vec<u8>>` per image/pdf flavor plus `Option<String>`
  text. It has its OWN static, separate from the file-URL `ClipboardEntry`, so a content paste never clobbers a pending
  file copy (pinned by `payload_tests::injecting_clipboard_data_does_not_clobber_the_file_url_entry`).
  `read_clipboard_data` is the read side (used by both the `playwright-e2e` mock module and the prod
  `CMDR_CLIPBOARD_BACKEND=mock` env path). `write_clipboard_data` / `clear_clipboard_data` are `#[cfg(test)]`
  unit-test-only injection (no prod / E2E caller exists yet; add an admin surface if E2E ever needs to inject content).
- **`pick_clipboard_payload(ClipboardData) -> ClipboardPayload`** applies flavor precedence: image (`public.png` >
  `public.tiff` > `public.jpeg`) > pdf (`com.adobe.pdf`) > text (`public.utf8-plain-text`). Real clipboards are
  multi-flavor (a Finder image copy carries the URL as text; a browser image copy carries the page URL), so we pick the
  highest-intent one.
- **Why TIFF→PNG**: macOS screenshots and many apps put `public.tiff` on the pasteboard. We convert it to PNG
  (`tiff_to_png`, via `NSBitmapImageRep` — a data class, so no main-thread requirement) and write `.png`, because a
  `.tiff` file is a poor default (large, poorly supported). A failed decode falls through to the next flavor rather than
  writing a broken file. `public.png` is written verbatim (no re-encode); `public.jpeg` is written verbatim as `.jpg`
  (no recompression); `com.adobe.pdf` verbatim as `.pdf`.
- **Markdown sniff** (`looks_like_markdown`, conservative): text becomes `.md` only on a strong signal (fenced code
  block, or an ATX heading at line start) or ≥2 DISTINCT weak signal KINDS (link, emphasis pair, list marker,
  blockquote) — "distinct" is by KIND, so two links are one kind. When in doubt it stays `.txt`; a wrong `.md` guess is
  worse than a plain `.txt`.
- **The read path** (`pasteboard::read_pasteboard_data`) does the minimum on the main thread: NSPasteboard is
  main-thread-only, so it just copies each flavor's raw bytes (`dataForType:` per UTI + `stringForType:` for text) into a
  `ClipboardData`. Flavor precedence and the TIFF→PNG decode (`pick_clipboard_payload`, hundreds of ms on a big image)
  run OFF the main thread, in a `spawn_blocking` in the command, so the UI never janks (principle 3). The mock/env path
  returns the injected `ClipboardData`; both feed the same `pick`, so precedence is identical across configurations.
- **The write** is `paste_file.rs` (`write_payload_to_dir`): it maps the payload to content and hands it to the
  engine's numbered-create instant op (`crates/cmdr-operations/src/create_numbered.rs`), whose docs carry the write
  rules. The command (`commands/clipboard.rs::paste_clipboard_as_file`, its macOS half in `platform/macos.rs`) reads the raw flavors on the main thread,
  picks/converts off-main, then hands the payload to `write_payload_replying`: a slow write answers `stillRunning`
  and reports its end (the file, or the typed refusal) on `clipboard-paste-settled`, never a timeout that drops the
  write. The reply and event types (`PasteClipboardReply`, `ClipboardPasteSettled`) live in `mod.rs` beside
  `PastedClipboardFile`, so the non-macOS stub shares them.
